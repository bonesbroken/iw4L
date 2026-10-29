use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::state::{CanvasAspect, DrawAction, DrawState, DrawTool, PALETTE};

pub(crate) const TOOLBAR_HEIGHT: f32 = 128.0;

#[derive(Component)]
pub(crate) struct DrawButton(pub(crate) DrawAction);

#[derive(Component)]
pub(crate) struct DrawRoot;

#[derive(Component)]
pub(crate) struct DrawStatus;

#[derive(Component)]
pub(crate) struct DrawCoords;

#[derive(Component)]
pub(crate) struct DrawPromptRoot;

#[derive(Component)]
pub(crate) struct DrawPromptText;

type Signature = (
    DrawTool,
    CanvasAspect,
    Option<usize>,
    [u8; 4],
    u32,
    usize,
    usize,
);

pub(crate) fn button_action(
    activation: On<bevy::ui_widgets::Activate>,
    buttons: Query<&DrawButton>,
    mut state: ResMut<DrawState>,
) {
    if let Ok(button) = buttons.get(activation.entity) {
        state.actions.push(button.0);
    }
}

pub(crate) fn draw_prompt(
    mut commands: Commands,
    state: Res<DrawState>,
    font: Res<ui::GameUiFont>,
    roots: Query<Entity, With<DrawPromptRoot>>,
    mut labels: Query<&mut Text, With<DrawPromptText>>,
    mut current: Local<Option<String>>,
) {
    let message = if state.carrying {
        Some("E  place canvas".to_owned())
    } else if state.pocket {
        Some("P  put away pocket canvas".to_owned())
    } else if state.near_canvas && !state.active {
        Some("Press E to draw   ·   Shift+E  move".to_owned())
    } else {
        None
    };
    let Some(message) = message else {
        for root in &roots {
            commands.entity(root).despawn();
        }
        *current = None;
        return;
    };
    if roots.is_empty() {
        commands
            .spawn((
                DrawPromptRoot,
                ui::UiLayer::Overlay,
                ui::UiLayerVisibility,
                GlobalZIndex(44),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    bottom: Val::Px(72.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|root| {
                root.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(7.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.025, 0.03, 0.82)),
                    BorderColor::all(Color::srgba(0.9, 0.72, 0.3, 0.7)),
                    Pickable::IGNORE,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        DrawPromptText,
                        Text::new(message.clone()),
                        ui::game_text_font(&font.0, 16.0),
                        TextColor(Color::srgb(0.96, 0.78, 0.34)),
                        Pickable::IGNORE,
                    ));
                });
            });
        *current = Some(message);
        return;
    }
    if current.as_ref() != Some(&message) {
        *current = Some(message.clone());
        for mut text in &mut labels {
            text.0 = message.clone();
        }
    }
}

pub(crate) fn draw_toolbar(
    mut commands: Commands,
    state: Res<DrawState>,
    font: Res<ui::GameUiFont>,
    roots: Query<Entity, With<DrawRoot>>,
    mut status: Query<&mut Text, (With<DrawStatus>, Without<DrawCoords>)>,
    mut coords: Query<&mut Text, (With<DrawCoords>, Without<DrawStatus>)>,
    mut signature: Local<Option<Signature>>,
) {
    if !state.active {
        for root in &roots {
            commands.entity(root).despawn();
        }
        *signature = None;
        return;
    }

    let wanted: Signature = (
        state.tool,
        state.preset,
        state.color_index,
        state.color,
        state.brush,
        state.undo.len(),
        state.redo.len(),
    );
    if !roots.is_empty() && signature.as_ref() == Some(&wanted) {
        update_labels(&state, &mut status, &mut coords);
        return;
    }
    *signature = Some(wanted);
    for root in &roots {
        commands.entity(root).despawn();
    }

    commands
        .spawn((
            DrawRoot,
            ui::UiLayer::Overlay,
            ui::UiLayerVisibility,
            GlobalZIndex(45),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(TOOLBAR_HEIGHT),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.018, 0.022, 0.88)),
        ))
        .with_children(|root| {
            root.spawn(Node {
                column_gap: Val::Px(5.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                text(
                    row,
                    &format!("DRAW · {} · {}px", state.tool.label(), state.brush),
                    &font.0,
                    16.0,
                    Color::srgb(0.96, 0.72, 0.28),
                );
                for tool in DrawTool::ALL {
                    button(
                        row,
                        tool.label(),
                        DrawAction::Tool(tool),
                        &font.0,
                        tool == state.tool,
                    );
                }
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                row.spawn((
                    DrawStatus,
                    Text::new(if state.status.is_empty() {
                        "Paint the board".to_owned()
                    } else {
                        state.status.clone()
                    }),
                    ui::game_text_font(&font.0, 14.0),
                    TextColor(Color::srgb(0.82, 0.9, 0.82)),
                    Pickable::IGNORE,
                ));
            });

            root.spawn(Node {
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                for (index, (_, color)) in PALETTE.iter().enumerate() {
                    swatch(row, index, *color, state.color_index == Some(index));
                }
                row.spawn((
                    Node {
                        width: Val::Px(30.0),
                        height: Val::Px(22.0),
                        margin: UiRect::left(Val::Px(6.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(
                        f32::from(state.color[0]) / 255.0,
                        f32::from(state.color[1]) / 255.0,
                        f32::from(state.color[2]) / 255.0,
                        1.0,
                    )),
                    BorderColor::all(Color::srgb(0.7, 0.7, 0.7)),
                    Pickable::IGNORE,
                ));
                text(
                    row,
                    &format!("Size {}", state.brush),
                    &font.0,
                    13.0,
                    Color::WHITE,
                );
                button(row, "-", DrawAction::Brush(-1), &font.0, false);
                button(row, "+", DrawAction::Brush(1), &font.0, false);
                for aspect in CanvasAspect::ALL {
                    button(
                        row,
                        aspect.label(),
                        DrawAction::Aspect(aspect),
                        &font.0,
                        aspect == state.preset,
                    );
                }
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                row.spawn((
                    DrawCoords,
                    Text::new(coords_text(&state)),
                    ui::game_text_font(&font.0, 13.0),
                    TextColor(Color::srgb(0.72, 0.78, 0.86)),
                    Pickable::IGNORE,
                ));
            });

            root.spawn(Node {
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                button(row, "Undo", DrawAction::Undo, &font.0, false);
                button(row, "Redo", DrawAction::Redo, &font.0, false);
                button(row, "Clear", DrawAction::Clear, &font.0, false);
                button(row, "Save PNG", DrawAction::Save, &font.0, false);
                button(row, "Exit", DrawAction::Exit, &font.0, false);
                text(
                    row,
                    "   LMB paint · RMB erase · B/L/R/F/E/I tools · 1-8 colors · [ ] size · \
                     Ctrl+Z/Y undo/redo · Ctrl+S save · Esc exit",
                    &font.0,
                    11.0,
                    Color::srgb(0.6, 0.64, 0.7),
                );
            });
        });

    update_labels(&state, &mut status, &mut coords);
}

fn update_labels(
    state: &DrawState,
    status: &mut Query<&mut Text, (With<DrawStatus>, Without<DrawCoords>)>,
    coords: &mut Query<&mut Text, (With<DrawCoords>, Without<DrawStatus>)>,
) {
    let message = if state.status.is_empty() {
        "Paint the board".to_owned()
    } else {
        state.status.clone()
    };
    for mut text in status.iter_mut() {
        if text.0 != message {
            text.0 = message.clone();
        }
    }
    let coordinates = coords_text(state);
    for mut text in coords.iter_mut() {
        if text.0 != coordinates {
            text.0 = coordinates.clone();
        }
    }
}

fn coords_text(state: &DrawState) -> String {
    let (width, height) = state.canvas.as_ref().map_or_else(
        || state.preset.dimensions(),
        |canvas| (canvas.width, canvas.height),
    );
    match state.cursor {
        Some((x, y)) => format!("x {x}  y {y}  ·  {width}×{height}"),
        None => format!("{width}×{height}"),
    }
}

fn text(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    font: &Handle<Font>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(value),
        ui::game_text_font(font, size),
        TextColor(color),
        Pickable::IGNORE,
    ));
}

fn button(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    action: DrawAction,
    font: &Handle<Font>,
    selected: bool,
) {
    parent
        .spawn((
            Button,
            Hovered::default(),
            DrawButton(action),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(if selected {
                Color::srgb(0.72, 0.42, 0.08)
            } else {
                Color::srgb(0.12, 0.14, 0.17)
            }),
            BorderColor::all(if selected {
                Color::srgb(0.98, 0.78, 0.32)
            } else {
                Color::srgb(0.28, 0.31, 0.36)
            }),
        ))
        .with_children(|parent| text(parent, title, font, 13.0, Color::WHITE));
}

fn swatch(parent: &mut ChildSpawnerCommands, index: usize, color: [u8; 4], selected: bool) {
    parent.spawn((
        Button,
        Hovered::default(),
        DrawButton(DrawAction::Color(index)),
        Node {
            width: Val::Px(26.0),
            height: Val::Px(22.0),
            border: UiRect::all(Val::Px(if selected { 2.0 } else { 1.0 })),
            ..default()
        },
        BackgroundColor(Color::srgba(
            f32::from(color[0]) / 255.0,
            f32::from(color[1]) / 255.0,
            f32::from(color[2]) / 255.0,
            1.0,
        )),
        BorderColor::all(if selected {
            Color::WHITE
        } else {
            Color::srgb(0.22, 0.24, 0.27)
        }),
    ));
}
