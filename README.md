# Lifthrasir Editor

GLB-native editor for converted Lifthrasir maps. It does not read or write
Ragnarok `RSW`, `GND`, `GAT`, or `GRF` files.

## Setup

The editor requires an `assets/` directory with `loader.toml`, converted maps/models, shaders, textures, and a pak.
Assets are not included in this repository.

To reuse assets from Lifthrasir you can make a symlink:

```bash
ln -s ../lifthrasir/assets assets
```

Or create `assets/`, copy `loader.example.toml` to `assets/loader.toml`, and
provide the referenced content. Maps must be loose `.glb` files inside
`assets/` to be opened and edited. Saving updates the selected GLB in place, so
back it up first.

Run the editor:

```bash
cargo run 
```

Optionally open a map at launch, relative to `assets/`:

```bash
cargo run -- --map data/maps/prontera/prontera.glb
```

The map must use the current `LIF_map` format and include the `LIF_editor`
payload. Convert outdated maps in the Lifthrasir checkout before opening them.

## Controls

### File and camera

- `Ctrl+O`: open a map
- `Ctrl+R`: reload the current map
- `Ctrl+S`: save the current map
- `W/A/S/D`: move the camera
- Right mouse drag: look around
- Mouse wheel: adjust camera speed
- `Space` / `Left Alt`: move camera up / down
- `P`: toggle the terrain grid
- `Ctrl+Z` / `Ctrl+Y`: undo / redo

Edits stay in memory until **Ctrl+S** (or **File → Save File**) is used; there
is no autosave.

### Collision

- Left click terrain: select a cell
- Arrow keys: selected cell another cell
- `F`: toggle the selected cell's walkability

### Props

- Click a prop: select it
- Drag a gizmo axis: transform the selected prop
- `G` / `R`: translate / rotate gizmo mode
- `I/K`, `J/L`, `U/O`: move along Z, X, Y
- `Q/E`: rotate around Y
- Use **Prop Transform** to snap gizmo drags, enter transforms numerically, or reset position, rotation, and scale.
- `Ctrl+D`: duplicate the selected prop
- `Delete`: delete the selected prop
- `Ctrl+Z` / `Ctrl+Y`: undo / redo

## Dependencies

Core document support uses `lifthrasir-data`. The GUI also uses Bevy, Dear
ImGui, `lifthrasir-assets`, and `lifthrasir-runtime`. The three Lifthrasir
crates are pinned to the same upstream revision so the editor's document schema
and renderer stay compatible.

## Roadmap

Status reflects the current implementation: checked items are complete; partial
items name the work that remains.

### 1. Editing reliability
- [x] Add structural tests for deleted-node GLB compaction and scene-index remapping.
- [x] Add a reload command and unsaved-change confirmation.
- [x] Make save errors preserve the in-memory document and clearly identify the affected path.
- [x] Ensure every edit type participates in the same coalesced history model.

### 2. Prop authoring
- [x] Add a mouse transform gizmo with translate and rotate modes.
- [x] Add snapping, numeric transform fields, and reset actions. Axis-constrained dragging is implemented.
- [ ] Add prop metadata panel: node name, model path, transform, animation values.
- [x] Add duplicate prop operations.
- [ ] Add replace-model operations.
- [ ] Add a searchable converted-model browser.

### 3. Terrain and collision
- [ ] Render a reliable collision grid overlay using the map's actual terrain coordinate conversion.
- [ ] Add brush selection and multi-cell collision editing.
- [ ] Add terrain height sculpting and regenerate terrain mesh data.
- [ ] Add terrain texture/material painting and preserve editor-owned terrain data in a new `LIF_editor` schema section.

### 4. World objects
- [ ] Edit lights, effects, and audio emitters.
- [ ] Add/delete emitters and edit their metadata.
- [ ] Add visual previews for effect and audio assets.
- [ ] Add map-level environment and lighting controls.

### 5. Asset management
- [ ] Scan map GLBs for referenced prop models.
- [ ] Provide a dry-run unused-model report.
- [ ] Add an explicit prune command with confirmation and backup support.
- [ ] Integrate edited maps into pak build validation.

### 6. Editor workflow
- [ ] Add multi-map project/open dialogs.
- [ ] Add editor camera bookmarks and focus-selected commands.
- [ ] Add selection outlines and a hierarchy/scene panel.
- [ ] Add import/export validation reports.
- [ ] Add snapshot/backup history for map documents.
