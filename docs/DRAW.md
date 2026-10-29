# Draw mode

`draw` opens a paint canvas on a wooden easel. Walking up to it shows **Press E
to draw**; opening the editor aims the fixed camera at the centre of the board,
releases the pointer, and hides the weapon and HUD. **Shift+E** moves the easel
instead. Closing the editor restores everything.

```text
make map mp_boneyard CMDS='wait world; spawn 0; force_match_start; draw'
iw4l map mp_boneyard --cmds 'wait world; spawn 0; force_match_start; draw'
```

| tool | key | note |
|---|---|---|
| Brush | B | freehand, left button |
| Line | L | drag to preview, release to keep |
| Rect | R | drag to preview, release to keep |
| Fill | F | flood fills the clicked region |
| Eraser | E | paints the paper colour; right button erases with any tool |
| Picker | I | copies the pixel colour under the cursor |

`1`-`8` select the palette, `[` / `]` and the wheel change the brush size.
`Ctrl+Z` / `Ctrl+Y` undo and redo (16 steps), `Ctrl+N` clears, `Ctrl+S` saves a
PNG, and `Esc` or `F10` closes the editor. The toolbar exposes the same actions.
Painting only happens while the cursor is over the canvas.

The toolbar switches between `1:1` (1024×1024), `16:9` (1024×576, default) and
`9:16` (576×1024). The picture is fitted inside the new shape without stretching;
the undo history restarts for the new canvas.

## Moving and pocket canvas

Walk up to the easel and press **Shift+E** to lift it; it follows the view, and
**E** sets it down. **P** (or `draw pocket on`) puts the canvas in your hands as a
small framed board and hides the weapon so you can walk around with it; **P**
again puts it back. The pocket board is the same picture. To paint it, use
`draw` (the easel returns while editing), then **P** to hold the updated picture.

```text
draw move      draw place      draw pocket on|off
```

## Scripted painting

Saved files land in `iw4l-artifacts/draw/` (`draw-<unix>.png` by default).

```text
draw save NAME            draw dot X Y
draw line X0 Y0 X1 Y1     draw rect X0 Y0 X1 Y1
draw color 3              draw color 255 128 0
draw size 20              draw aspect 16:9
draw new
```

Coordinates are canvas pixels; the toolbar shows the current width and height.
`draw new` starts a fresh canvas and re-places the easel from the current view;
`draw off` or `draw exit` leaves the picture in the map. A map change or match
teardown discards it; `draw save` before leaving keeps it.
