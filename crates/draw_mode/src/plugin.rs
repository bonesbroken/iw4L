use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use frame::{
    AppScreen, ClientSet, HudDraw, LaunchIdentity, UiOverlayInput, UiPointerFree, WorldGeneration,
};
use render_scene::{CameraView, FlyCamera, FpvLens, SimCamera};

use crate::canvas;
use crate::state::{CanvasAspect, DrawAction, DrawState, DrawTool, PALETTE, PAPER, SavedResources};

pub struct DrawModePlugin;

impl Plugin for DrawModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrawState>()
            .init_resource::<frame::DrawCanvas>()
            .init_resource::<frame::ViewmodelVisible>()
            .add_systems(Startup, register_command)
            .add_systems(First, sync_world)
            .add_systems(
                PreUpdate,
                apply_ui_capture.before(bevy::input::InputSystems),
            )
            .add_systems(Update, apply_mode.in_set(ClientSet::Input))
            .add_systems(
                Update,
                (
                    route_commands,
                    hotkeys,
                    carry_interaction,
                    apply_actions,
                    paint,
                )
                    .chain()
                    .in_set(ClientSet::Diag)
                    .after(console::ConsoleDispatchSet),
            )
            .add_systems(
                Update,
                (crate::ui::draw_toolbar, crate::ui::draw_prompt)
                    .chain()
                    .in_set(ClientSet::Ui),
            )
            .add_systems(
                PostUpdate,
                freeze_lens.before(bevy::transform::TransformSystems::Propagate),
            )
            .add_observer(crate::ui::button_action)
            .add_systems(OnEnter(frame::MatchScope::Absent), reset_match);
    }
}

fn register_command(mut registry: ResMut<console::ConsoleRegistry>) {
    registry.register(console::CommandSpec::new("draw").usage(
        "draw [on|off|new|clear|undo|redo|save NAME|size N|color 1-8|color R G B|aspect 1:1|16:9|9:16|dot X Y|line X0 Y0 X1 Y1|rect X0 Y0 X1 Y1|move|place|pocket [on|off]] \
         — canvas on an easel or in hand",
    ));
}

#[derive(Clone, Copy)]
struct CameraNow {
    transform: Transform,
    vertical_fov: f32,
    aspect: f32,
}

fn current_camera(
    poses: &Query<&GlobalTransform, With<FlyCamera>>,
    lenses: &Query<&Projection, With<FpvLens>>,
) -> Option<CameraNow> {
    let transform = poses.iter().next()?.compute_transform();
    let projection = lenses.iter().next()?;
    let (vertical_fov, aspect) = match projection {
        Projection::Perspective(perspective) => (perspective.fov, perspective.aspect_ratio),
        _ => (60.0_f32.to_radians(), 16.0 / 9.0),
    };
    Some(CameraNow {
        transform,
        vertical_fov,
        aspect: if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            16.0 / 9.0
        },
    })
}

fn target_pixels(state: &DrawState, draw: &mut frame::DrawCanvas) {
    if let (Some(plane), Some(pixels)) = (state.plane, state.canvas.clone()) {
        draw.show(
            plane.origin.to_array(),
            plane.right.to_array(),
            plane.up.to_array(),
            pixels,
        );
        draw.overlay = state.active || state.pocket;
        draw.pocket = state.pocket;
    }
}

fn activate(
    state: &mut DrawState,
    draw: &mut frame::DrawCanvas,
    camera: CameraNow,
    mut saved: SavedResources,
    viewmodel: &mut frame::ViewmodelVisible,
) {
    if state.pocket {
        state.exit_pocket();
        viewmodel.0 = true;
        saved.viewmodel = true;
    }
    if !state.active {
        state.saved = saved;
    }
    state.aspect = camera.aspect;
    state.ensure_plane(camera.transform, camera.vertical_fov);
    state.focus_plane(camera.vertical_fov);
    state.canvas();
    state.active = true;
    state.carrying = false;
    state.drag = None;
    state.cursor = None;
    state.actions.clear();
    state.status = "Paint the board".into();
    state.ui_revision = state.ui_revision.wrapping_add(1);
    target_pixels(state, draw);
    let (width, height) = state.preset.dimensions();
    diag::info!(
        Ui,
        "draw: mode on canvas={}x{} aspect={} origin={:?}",
        width,
        height,
        state.preset.label(),
        state.plane.map(|plane| plane.origin.to_array())
    );
}

fn deactivate(
    state: &mut DrawState,
    hud: &mut HudDraw,
    pointer: &mut UiPointerFree,
    overlay: &mut UiOverlayInput,
    viewmodel: &mut frame::ViewmodelVisible,
    camera: &mut SimCamera,
) {
    if !state.active {
        return;
    }
    state.active = false;
    state.drag = None;
    state.cursor = None;
    state.status = "Draw mode off".into();
    state.ui_revision = state.ui_revision.wrapping_add(1);
    hud.0 = state.saved.hud;
    pointer.0 = state.saved.pointer;
    overlay.0 = state.saved.overlay;
    viewmodel.0 = state.saved.viewmodel;
    camera.view_override = state.saved.view_override;
    diag::info!(Ui, "draw: mode off");
}

/// The viewmodel pass writes a lens offset into the camera child every frame.
/// With the viewmodel hidden that offset is only a camera kick; zero it while
/// editing so the fixed view and the board share one pose.
fn freeze_lens(state: Res<DrawState>, mut lenses: Query<&mut Transform, With<FpvLens>>) {
    if !state.active && !state.pocket {
        return;
    }
    for mut transform in &mut lenses {
        if *transform != Transform::IDENTITY {
            *transform = Transform::IDENTITY;
        }
    }
}

/// `+activate` / `+usereload` on the keyboard. The canvas uses the same key a
/// sentry placement would: close enough, press use to lift it; use again to set
/// it down.
#[allow(clippy::too_many_arguments)]
fn carry_interaction(
    mut state: ResMut<DrawState>,
    mut draw: ResMut<frame::DrawCanvas>,
    hud: Res<HudDraw>,
    pointer: Res<UiPointerFree>,
    overlay: Res<UiOverlayInput>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
    sim_camera: Res<SimCamera>,
    screen: Res<AppScreen>,
    console: Res<console::ConsoleState>,
    keys: Res<ButtonInput<KeyCode>>,
    poses: Query<&GlobalTransform, With<FlyCamera>>,
    lenses: Query<&Projection, With<FpvLens>>,
) {
    if state.active || *screen != AppScreen::InGame || console.open {
        if state.active {
            state.near_canvas = false;
        }
        return;
    }
    let Some(camera) = current_camera(&poses, &lenses) else {
        state.near_canvas = false;
        return;
    };
    if state.pocket {
        state.aspect = camera.aspect;
        state.place_pocket(camera.transform, camera.vertical_fov);
        target_pixels(&state, &mut draw);
        if keys.just_pressed(KeyCode::KeyP) {
            state.exit_pocket();
            viewmodel.0 = true;
            target_pixels(&state, &mut draw);
            diag::info!(Ui, "draw: pocket canvas away");
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        state.aspect = camera.aspect;
        state.enter_pocket(camera.transform, camera.vertical_fov);
        viewmodel.0 = false;
        target_pixels(&state, &mut draw);
        diag::info!(Ui, "draw: pocket canvas out");
        return;
    }
    if state.carrying {
        state.aspect = camera.aspect;
        state.follow_camera(camera.transform, camera.vertical_fov);
        target_pixels(&state, &mut draw);
        if keys.just_pressed(KeyCode::KeyE) {
            state.place_carry();
            target_pixels(&state, &mut draw);
            diag::info!(Ui, "draw: canvas placed");
        }
        return;
    }
    let Some(plane) = state.plane else {
        state.near_canvas = false;
        return;
    };
    let reach = (plane.up.length().max(plane.right.length()) + 80.0).max(130.0);
    let distance = camera.transform.translation.distance(plane.origin);
    state.near_canvas = distance <= reach;
    if state.near_canvas && keys.just_pressed(KeyCode::KeyE) {
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if shift {
            state.aspect = camera.aspect;
            state.start_carry(camera.transform, camera.vertical_fov);
            target_pixels(&state, &mut draw);
            diag::info!(Ui, "draw: canvas picked up");
        } else {
            let saved = SavedResources {
                hud: hud.0,
                pointer: pointer.0,
                overlay: overlay.0,
                viewmodel: viewmodel.0,
                view_override: sim_camera.view_override,
            };
            activate(&mut state, &mut draw, camera, saved, &mut viewmodel);
        }
    }
}

/// The pointer must be free before the console's `sync_cursor_grab` runs in
/// `PreUpdate`; theatre's HUD sync in `First` writes the same resources back to
/// their gameplay values, so the capture is republished here every frame.
fn apply_ui_capture(
    state: Res<DrawState>,
    screen: Res<AppScreen>,
    mut hud: ResMut<HudDraw>,
    mut pointer: ResMut<UiPointerFree>,
    mut overlay: ResMut<UiOverlayInput>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
) {
    if *screen != AppScreen::InGame {
        return;
    }
    if state.active {
        hud.0 = false;
        pointer.0 = true;
        overlay.0 = true;
        viewmodel.0 = false;
    } else if state.pocket {
        viewmodel.0 = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_mode(
    mut state: ResMut<DrawState>,
    mut draw: ResMut<frame::DrawCanvas>,
    mut hud: ResMut<HudDraw>,
    mut pointer: ResMut<UiPointerFree>,
    mut overlay: ResMut<UiOverlayInput>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
    mut camera: ResMut<SimCamera>,
    screen: Res<AppScreen>,
    lenses: Query<&Projection, With<FpvLens>>,
    mut was_active: Local<bool>,
) {
    if state.active && *screen != AppScreen::InGame {
        state.active = false;
    }
    if state.active {
        if let Some(Projection::Perspective(perspective)) = lenses.iter().next() {
            let aspect = perspective.aspect_ratio;
            if aspect.is_finite() && aspect > 0.0 {
                state.aspect = aspect;
            }
        }
        let horizontal_fov = 2.0
            * ((state.vertical_fov * 0.5).tan() / hud_iw4::CG_TANHALF_FOV_Y_SCALE)
                .atan()
                .to_degrees();
        if let Some(transform) = state.camera {
            camera.view_override = Some(CameraView {
                transform,
                horizontal_fov,
            });
        }
        hud.0 = false;
        pointer.0 = true;
        overlay.0 = true;
        viewmodel.0 = false;
        target_pixels(&state, &mut draw);
        *was_active = true;
    } else if *was_active {
        *was_active = false;
        draw.overlay = false;
        hud.0 = state.saved.hud;
        pointer.0 = state.saved.pointer;
        overlay.0 = state.saved.overlay;
        viewmodel.0 = state.saved.viewmodel;
        camera.view_override = state.saved.view_override;
    }
}

fn sync_world(
    generation: Option<Res<WorldGeneration>>,
    mut state: ResMut<DrawState>,
    mut draw: ResMut<frame::DrawCanvas>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
    mut last: Local<Option<u64>>,
) {
    let Some(generation) = generation else {
        return;
    };
    let id = generation.id().get();
    if last.is_some_and(|previous| previous != id) {
        state.reset();
        draw.clear();
        viewmodel.0 = true;
    }
    *last = Some(id);
}

fn reset_match(
    mut state: ResMut<DrawState>,
    mut draw: ResMut<frame::DrawCanvas>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
) {
    state.reset();
    draw.clear();
    viewmodel.0 = true;
}

#[allow(clippy::too_many_arguments)]
fn route_commands(
    mut events: MessageReader<console::ConsoleCommand>,
    mut state: ResMut<DrawState>,
    mut draw: ResMut<frame::DrawCanvas>,
    mut hud: ResMut<HudDraw>,
    mut pointer: ResMut<UiPointerFree>,
    mut overlay: ResMut<UiOverlayInput>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
    mut camera: ResMut<SimCamera>,
    screen: Res<AppScreen>,
    identity: Option<Res<LaunchIdentity>>,
    poses: Query<&GlobalTransform, With<FlyCamera>>,
    lenses: Query<&Projection, With<FpvLens>>,
    (mut console_state, settings, mut line): (
        ResMut<console::ConsoleState>,
        Res<console::ConsoleSettings>,
        ResMut<console::ConsoleLine>,
    ),
) {
    let capacity = settings.log_capacity;
    let echo =
        |msg: String, console: &mut console::ConsoleState, line: &mut console::ConsoleLine| {
            diag::info!(Ui, "{msg}");
            line.0 = msg.clone();
            console.echo(msg, capacity);
        };

    for command in events.read() {
        if command.name != "draw" {
            continue;
        }
        let args: Vec<&str> = command.args.iter().map(String::as_str).collect();
        if !matches!(args.as_slice(), ["save", ..]) && *screen != AppScreen::InGame {
            echo(
                "draw: only in a live match".into(),
                &mut console_state,
                &mut line,
            );
            continue;
        }
        match args.as_slice() {
            [] => {
                if state.active {
                    deactivate(
                        &mut state,
                        &mut hud,
                        &mut pointer,
                        &mut overlay,
                        &mut viewmodel,
                        &mut camera,
                    );
                } else if let Some(now) = current_camera(&poses, &lenses) {
                    let saved = SavedResources {
                        hud: hud.0,
                        pointer: pointer.0,
                        overlay: overlay.0,
                        viewmodel: viewmodel.0,
                        view_override: camera.view_override,
                    };
                    activate(&mut state, &mut draw, now, saved, &mut viewmodel);
                } else {
                    echo(
                        "draw: player camera unavailable".into(),
                        &mut console_state,
                        &mut line,
                    );
                }
            }
            ["on"] | ["enter"] => {
                if let Some(now) = current_camera(&poses, &lenses) {
                    let saved = SavedResources {
                        hud: hud.0,
                        pointer: pointer.0,
                        overlay: overlay.0,
                        viewmodel: viewmodel.0,
                        view_override: camera.view_override,
                    };
                    activate(&mut state, &mut draw, now, saved, &mut viewmodel);
                } else {
                    echo(
                        "draw: player camera unavailable".into(),
                        &mut console_state,
                        &mut line,
                    );
                }
            }
            ["off"] | ["exit"] => {
                deactivate(
                    &mut state,
                    &mut hud,
                    &mut pointer,
                    &mut overlay,
                    &mut viewmodel,
                    &mut camera,
                );
            }
            ["move"] | ["carry"] => {
                if state.active {
                    echo(
                        "draw: exit the editor before moving the canvas".into(),
                        &mut console_state,
                        &mut line,
                    );
                } else if let Some(now) = current_camera(&poses, &lenses) {
                    if state.pocket {
                        state.exit_pocket();
                        viewmodel.0 = true;
                    }
                    state.aspect = now.aspect;
                    state.start_carry(now.transform, now.vertical_fov);
                    target_pixels(&state, &mut draw);
                } else {
                    echo(
                        "draw: player camera unavailable".into(),
                        &mut console_state,
                        &mut line,
                    );
                }
            }
            ["place"] => {
                if state.carrying {
                    state.place_carry();
                    target_pixels(&state, &mut draw);
                }
            }
            ["new"] => {
                if state.pocket {
                    state.exit_pocket();
                    viewmodel.0 = true;
                }
                let Some(now) = current_camera(&poses, &lenses) else {
                    echo(
                        "draw: player camera unavailable".into(),
                        &mut console_state,
                        &mut line,
                    );
                    continue;
                };
                state.new_canvas();
                state.place_from_camera(now.transform, now.vertical_fov);
                let saved = SavedResources {
                    hud: hud.0,
                    pointer: pointer.0,
                    overlay: overlay.0,
                    viewmodel: viewmodel.0,
                    view_override: camera.view_override,
                };
                activate(&mut state, &mut draw, now, saved, &mut viewmodel);
            }
            ["pocket"] => {
                if state.active {
                    echo(
                        "draw: exit the editor before the pocket canvas".into(),
                        &mut console_state,
                        &mut line,
                    );
                } else if state.pocket {
                    state.exit_pocket();
                    viewmodel.0 = true;
                    target_pixels(&state, &mut draw);
                } else if let Some(now) = current_camera(&poses, &lenses) {
                    state.aspect = now.aspect;
                    state.enter_pocket(now.transform, now.vertical_fov);
                    viewmodel.0 = false;
                    target_pixels(&state, &mut draw);
                } else {
                    echo(
                        "draw: player camera unavailable".into(),
                        &mut console_state,
                        &mut line,
                    );
                }
            }
            ["pocket", "on"] => {
                if state.active {
                    echo(
                        "draw: exit the editor before the pocket canvas".into(),
                        &mut console_state,
                        &mut line,
                    );
                } else if !state.pocket {
                    if let Some(now) = current_camera(&poses, &lenses) {
                        state.aspect = now.aspect;
                        state.enter_pocket(now.transform, now.vertical_fov);
                        viewmodel.0 = false;
                        target_pixels(&state, &mut draw);
                    } else {
                        echo(
                            "draw: player camera unavailable".into(),
                            &mut console_state,
                            &mut line,
                        );
                    }
                }
            }
            ["pocket", "off"] => {
                if state.pocket {
                    state.exit_pocket();
                    viewmodel.0 = true;
                    target_pixels(&state, &mut draw);
                }
            }
            ["clear"] => state.clear(),
            ["undo"] => state.undo(),
            ["redo"] => state.redo(),
            ["dot", x, y] => match (x.parse::<i32>(), y.parse::<i32>()) {
                (Ok(x), Ok(y)) => script_dot(&mut state, x, y),
                _ => echo(
                    "draw: dot needs X Y in canvas pixels".into(),
                    &mut console_state,
                    &mut line,
                ),
            },
            ["line", x0, y0, x1, y1] => match (
                x0.parse::<i32>(),
                y0.parse::<i32>(),
                x1.parse::<i32>(),
                y1.parse::<i32>(),
            ) {
                (Ok(x0), Ok(y0), Ok(x1), Ok(y1)) => script_line(&mut state, (x0, y0), (x1, y1)),
                _ => echo(
                    "draw: line needs X0 Y0 X1 Y1 in canvas pixels".into(),
                    &mut console_state,
                    &mut line,
                ),
            },
            ["rect", x0, y0, x1, y1] => match (
                x0.parse::<i32>(),
                y0.parse::<i32>(),
                x1.parse::<i32>(),
                y1.parse::<i32>(),
            ) {
                (Ok(x0), Ok(y0), Ok(x1), Ok(y1)) => script_rect(&mut state, (x0, y0), (x1, y1)),
                _ => echo(
                    "draw: rect needs X0 Y0 X1 Y1 in canvas pixels".into(),
                    &mut console_state,
                    &mut line,
                ),
            },
            ["save"] => save_canvas(&mut state, identity.as_deref(), None),
            ["save", name] => save_canvas(&mut state, identity.as_deref(), Some(*name)),
            ["size", value] => match value.parse::<u32>() {
                Ok(size) => state.set_brush(size),
                Err(_) => echo(
                    "draw: size must be 1..=64".into(),
                    &mut console_state,
                    &mut line,
                ),
            },
            ["color", index] => match index.parse::<usize>() {
                Ok(index) if (1..=PALETTE.len()).contains(&index) => state.set_color(index - 1),
                _ => echo("draw: color is 1..=8".into(), &mut console_state, &mut line),
            },
            ["color", red, green, blue] => {
                match (red.parse::<u8>(), green.parse::<u8>(), blue.parse::<u8>()) {
                    (Ok(red), Ok(green), Ok(blue)) => {
                        state.set_custom_color([red, green, blue, 255])
                    }
                    _ => echo(
                        "draw: color needs R G B in 0..=255".into(),
                        &mut console_state,
                        &mut line,
                    ),
                }
            }
            ["aspect", value] => match CanvasAspect::from_arg(value) {
                Some(aspect) => {
                    state.set_aspect(aspect);
                    target_pixels(&state, &mut draw);
                }
                None => echo(
                    "draw: aspect must be square, 16:9 or 9:16".into(),
                    &mut console_state,
                    &mut line,
                ),
            },
            _ => echo(
                "usage: draw [on|off|new|clear|undo|redo|save NAME|size N|color 1-8|aspect 1:1|16:9|9:16]".into(),
                &mut console_state,
                &mut line,
            ),
        }
    }
}

fn script_dot(state: &mut DrawState, x: i32, y: i32) {
    state.push_undo();
    let Some(canvas) = state.canvas.clone() else {
        return;
    };
    let brush = state.brush;
    let color = state.color;
    canvas.edit(|pixels, width, height| {
        canvas::stamp(pixels, width, height, (x, y), brush, color);
    });
    state.status = format!("Dot {x}, {y}");
}

fn script_line(state: &mut DrawState, from: (i32, i32), to: (i32, i32)) {
    state.push_undo();
    let Some(canvas) = state.canvas.clone() else {
        return;
    };
    let brush = state.brush;
    let color = state.color;
    canvas.edit(|pixels, width, height| {
        canvas::line(pixels, width, height, from, to, brush, color);
    });
    state.status = format!("Line {:?} → {:?}", from, to);
}

fn script_rect(state: &mut DrawState, from: (i32, i32), to: (i32, i32)) {
    state.push_undo();
    let Some(canvas) = state.canvas.clone() else {
        return;
    };
    let brush = state.brush;
    let color = state.color;
    canvas.edit(|pixels, width, height| {
        canvas::rectangle(pixels, width, height, from, to, brush, color);
    });
    state.status = format!("Rect {:?} → {:?}", from, to);
}

fn save_canvas(state: &mut DrawState, identity: Option<&LaunchIdentity>, name: Option<&str>) {
    let Some(identity) = identity else {
        state.status = "draw: artifacts path unknown".into();
        return;
    };
    let Some(canvas_pixels) = state.canvas.clone() else {
        state.status = "draw: nothing to save".into();
        return;
    };
    let path = match canvas::canvas_path(&identity.artifacts, name) {
        Ok(path) => path,
        Err(error) => {
            state.status = format!("draw: {error}");
            return;
        }
    };
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        state.status = format!("draw: create {}: {error}", parent.display());
        return;
    }
    let pixels = canvas_pixels.read();
    match canvas::write_png(&path, &pixels, canvas_pixels.width, canvas_pixels.height) {
        Ok(()) => {
            state.status = format!(
                "Saved {}",
                path.strip_prefix(&identity.artifacts)
                    .unwrap_or(&path)
                    .display()
            );
            diag::info!(Ui, "draw: saved {}", path.display());
        }
        Err(error) => state.status = format!("draw: {error}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_actions(
    mut state: ResMut<DrawState>,
    mut draw: ResMut<frame::DrawCanvas>,
    mut hud: ResMut<HudDraw>,
    mut pointer: ResMut<UiPointerFree>,
    mut overlay: ResMut<UiOverlayInput>,
    mut viewmodel: ResMut<frame::ViewmodelVisible>,
    mut camera: ResMut<SimCamera>,
    identity: Option<Res<LaunchIdentity>>,
) {
    let actions = std::mem::take(&mut state.actions);
    for action in actions {
        match action {
            DrawAction::Tool(tool) => state.set_tool(tool),
            DrawAction::Aspect(aspect) => {
                state.set_aspect(aspect);
                target_pixels(&state, &mut draw);
            }
            DrawAction::Color(index) => state.set_color(index),
            DrawAction::Brush(delta) => state.nudge_brush(delta),
            DrawAction::Undo => state.undo(),
            DrawAction::Redo => state.redo(),
            DrawAction::Clear => state.clear(),
            DrawAction::Save => save_canvas(&mut state, identity.as_deref(), None),
            DrawAction::Exit => {
                deactivate(
                    &mut state,
                    &mut hud,
                    &mut pointer,
                    &mut overlay,
                    &mut viewmodel,
                    &mut camera,
                );
            }
        }
    }
}

fn hotkeys(
    mut state: ResMut<DrawState>,
    keys: Res<ButtonInput<KeyCode>>,
    console: Res<console::ConsoleState>,
    screen: Res<AppScreen>,
) {
    if !state.active || console.open || *screen != AppScreen::InGame {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::F10) {
        state.actions.push(DrawAction::Exit);
        return;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        state.set_tool(DrawTool::Brush);
    }
    if keys.just_pressed(KeyCode::KeyE) {
        state.set_tool(DrawTool::Eraser);
    }
    if keys.just_pressed(KeyCode::KeyL) {
        state.set_tool(DrawTool::Line);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        state.set_tool(DrawTool::Rect);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        state.set_tool(DrawTool::Fill);
    }
    if keys.just_pressed(KeyCode::KeyI) {
        state.set_tool(DrawTool::Picker);
    }
    for (key, index) in [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
        (KeyCode::Digit6, 5),
        (KeyCode::Digit7, 6),
        (KeyCode::Digit8, 7),
    ] {
        if keys.just_pressed(key) {
            state.set_color(index);
        }
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        state.nudge_brush(-1);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        state.nudge_brush(1);
    }
    if control && keys.just_pressed(KeyCode::KeyZ) {
        state.undo();
    }
    if control && keys.just_pressed(KeyCode::KeyY) {
        state.redo();
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        state.actions.push(DrawAction::Save);
    }
    if control && keys.just_pressed(KeyCode::KeyN) {
        state.actions.push(DrawAction::Clear);
    }
}

fn paint(
    mut state: ResMut<DrawState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    console: Res<console::ConsoleState>,
    screen: Res<AppScreen>,
) {
    if !state.active || console.open || *screen != AppScreen::InGame {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let mut wheel_delta = 0.0;
    for event in wheel.read() {
        wheel_delta += event.y;
    }
    if wheel_delta != 0.0 {
        state.nudge_brush(if wheel_delta > 0.0 { 1 } else { -1 });
    }

    if state.drag.is_some()
        && !buttons.pressed(MouseButton::Left)
        && !buttons.pressed(MouseButton::Right)
    {
        state.drag = None;
        state.status = format!("{} ready", state.tool.label());
        state.ui_revision = state.ui_revision.wrapping_add(1);
        return;
    }

    let Some(position) = window.cursor_position() else {
        return;
    };
    if position.y < crate::ui::TOOLBAR_HEIGHT {
        state.cursor = None;
        return;
    }
    let Some(pixel) = cursor_pixel(&state, window, position) else {
        state.cursor = None;
        return;
    };
    state.cursor = Some((pixel.0.max(0) as u32, pixel.1.max(0) as u32));

    if state.drag.is_none() {
        if buttons.just_pressed(MouseButton::Left) {
            begin_stroke(&mut state, pixel, false);
        } else if buttons.just_pressed(MouseButton::Right) {
            begin_stroke(&mut state, pixel, true);
        }
        return;
    }
    update_stroke(&mut state, pixel);
}

fn cursor_pixel(state: &DrawState, window: &Window, position: Vec2) -> Option<(i32, i32)> {
    let plane = state.plane?;
    let camera = state.camera?;
    let tan_vertical = (state.vertical_fov * 0.5).tan();
    let tan_horizontal = tan_vertical * state.aspect;
    let ndc_x = 2.0 * position.x / window.width().max(1.0) - 1.0;
    let ndc_y = 1.0 - 2.0 * position.y / window.height().max(1.0);
    let direction = (*camera.forward()
        + *camera.right() * ndc_x * tan_horizontal
        + *camera.up() * ndc_y * tan_vertical)
        .normalize();
    let uv = plane.uv(camera.translation, direction)?;
    let (width, height) = state.canvas.as_ref().map_or_else(
        || state.preset.dimensions(),
        |canvas| (canvas.width, canvas.height),
    );
    let x = (uv.x * width as f32).floor() as i32;
    let y = (uv.y * height as f32).floor() as i32;
    Some((x.clamp(0, width as i32 - 1), y.clamp(0, height as i32 - 1)))
}

fn begin_stroke(state: &mut DrawState, pixel: (i32, i32), erasing: bool) {
    let Some(canvas) = state.canvas.clone() else {
        return;
    };
    let tool = if erasing {
        DrawTool::Eraser
    } else {
        state.tool
    };
    let color = if erasing { PAPER } else { state.color };
    match tool {
        DrawTool::Picker => {
            let pixels = canvas.read();
            if let Some(picked) =
                canvas::pixel(&pixels, canvas.width, canvas.height, pixel.0, pixel.1)
            {
                drop(pixels);
                state.set_custom_color(picked);
                state.status = format!("Picked {}, {}, {}", picked[0], picked[1], picked[2]);
            }
        }
        DrawTool::Fill => {
            state.push_undo();
            canvas.edit(|pixels, width, height| {
                canvas::flood_fill(pixels, width, height, pixel, color);
            });
            state.status = "Filled".into();
            state.ui_revision = state.ui_revision.wrapping_add(1);
        }
        DrawTool::Brush | DrawTool::Eraser => {
            state.push_undo();
            let brush = state.brush;
            canvas.edit(|pixels, width, height| {
                canvas::stamp(pixels, width, height, pixel, brush, color);
            });
            state.drag = Some(crate::state::Drag {
                erasing,
                start: pixel,
                last: pixel,
                base: None,
            });
        }
        DrawTool::Line | DrawTool::Rect => {
            state.push_undo();
            let base = state.undo.last().cloned();
            state.drag = Some(crate::state::Drag {
                erasing,
                start: pixel,
                last: pixel,
                base,
            });
        }
    }
}

fn update_stroke(state: &mut DrawState, pixel: (i32, i32)) {
    let Some(mut drag) = state.drag.take() else {
        return;
    };
    if drag.last == pixel && !matches!(state.tool, DrawTool::Line | DrawTool::Rect) {
        state.drag = Some(drag);
        return;
    }
    let Some(canvas) = state.canvas.clone() else {
        return;
    };
    let color = if drag.erasing { PAPER } else { state.color };
    let brush = state.brush;
    let tool = if drag.erasing {
        DrawTool::Eraser
    } else {
        state.tool
    };
    match tool {
        DrawTool::Brush | DrawTool::Eraser => {
            let from = drag.last;
            canvas.edit(|pixels, width, height| {
                canvas::line(pixels, width, height, from, pixel, brush, color);
            });
        }
        DrawTool::Line => {
            if let Some(base) = drag.base.as_ref() {
                canvas.edit(|pixels, width, height| {
                    pixels.copy_from_slice(base);
                    canvas::line(pixels, width, height, drag.start, pixel, brush, color);
                });
            }
        }
        DrawTool::Rect => {
            if let Some(base) = drag.base.as_ref() {
                canvas.edit(|pixels, width, height| {
                    pixels.copy_from_slice(base);
                    canvas::rectangle(pixels, width, height, drag.start, pixel, brush, color);
                });
            }
        }
        DrawTool::Fill | DrawTool::Picker => {}
    }
    drag.last = pixel;
    state.drag = Some(drag);
}
