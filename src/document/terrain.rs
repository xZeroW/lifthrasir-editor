//! Editing the converted terrain mesh in place, without reconstructing GND.
use super::*;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct TerrainPrimitive {
    pub mesh_index: usize,
    pub primitive_index: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    indices: Vec<usize>,
    position_accessor: usize,
    normal_accessor: usize,
}

impl MapDocument {
    pub fn terrain_primitives(&self) -> Option<&[TerrainPrimitive]> {
        self.terrain.as_deref()
    }

    pub fn terrain_revision(&self) -> u64 {
        self.terrain_revision
    }

    /// Raises terrain vertices within the collision brush footprint by `delta`
    /// world units. Collision heights are intentionally independent. Boundary
    /// vertices are included across all materials to keep terrain seams joined.
    pub fn sculpt_terrain(&mut self, x: u32, y: u32, radius: u32, delta: f32) -> Result<usize> {
        ensure!(delta.is_finite(), "terrain height delta must be finite");
        ensure!(
            x < self.editor.collision.width && y < self.editor.collision.height,
            "terrain brush is outside the map"
        );
        ensure!(radius <= 16, "terrain brush radius must be at most 16");
        if delta == 0.0 {
            return Ok(0);
        }
        if self.terrain.is_none() {
            self.terrain = Some(read_terrain(&self.root, &self.bin_chunk)?);
        }
        let minimum_x = x.saturating_sub(radius) as f32 * 5.0;
        let maximum_x = (x.saturating_add(radius).saturating_add(1)) as f32 * 5.0;
        let minimum_z = -(y.saturating_add(radius).saturating_add(1) as f32) * 5.0;
        let maximum_z = -(y.saturating_sub(radius) as f32) * 5.0;
        let mut vertices = Vec::new();
        for (primitive_index, primitive) in self.terrain.as_ref().unwrap().iter().enumerate() {
            for (index, point) in primitive.positions.iter().enumerate() {
                if point[0] >= minimum_x - 0.001
                    && point[0] <= maximum_x + 0.001
                    && point[2] >= minimum_z - 0.001
                    && point[2] <= maximum_z + 0.001
                {
                    let height = point[1] + delta;
                    ensure!(height.is_finite(), "terrain height overflows");
                    vertices.push((primitive_index, index, point[1], height));
                }
            }
        }
        let count = vertices.len();
        if count == 0 {
            return Ok(0);
        }
        self.commit_pending_prop_transform();
        let changed: BTreeSet<_> = vertices.iter().map(|(primitive, ..)| *primitive).collect();
        let normals_before = changed
            .iter()
            .map(|&index| (index, self.terrain.as_ref().unwrap()[index].normals.clone()))
            .collect();
        self.apply_terrain_heights(&vertices, true);
        self.undo.push(DocumentEdit::TerrainHeight {
            vertices,
            normals_before,
        });
        self.redo.clear();
        self.dirty = true;
        Ok(count)
    }

    pub(super) fn apply_terrain_heights(
        &mut self,
        edits: &[(usize, usize, f32, f32)],
        forward: bool,
    ) {
        let terrain = self
            .terrain
            .as_mut()
            .expect("terrain history has loaded geometry");
        for &(primitive, vertex, before, after) in edits {
            terrain[primitive].positions[vertex][1] = if forward { after } else { before };
        }
        let changed: BTreeSet<_> = edits.iter().map(|(primitive, ..)| *primitive).collect();
        for index in changed {
            terrain[index].rebuild_normals();
        }
        self.terrain_revision = self.terrain_revision.wrapping_add(1);
    }

    pub(super) fn write_terrain(&self, root: &mut Value, bin: &mut [u8]) -> Result<()> {
        let Some(terrain) = &self.terrain else {
            return Ok(());
        };
        for primitive in terrain {
            for (accessor, values) in [
                (primitive.position_accessor, &primitive.positions),
                (primitive.normal_accessor, &primitive.normals),
            ] {
                let layout = accessor_layout(root, accessor, bin.len(), 5126, "VEC3", 12)?;
                for (index, point) in values.iter().enumerate() {
                    for (axis, value) in point.iter().enumerate() {
                        let offset = layout.start + index * layout.stride + axis * 4;
                        bin[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                    }
                }
                let mut minimum = [f32::INFINITY; 3];
                let mut maximum = [f32::NEG_INFINITY; 3];
                for point in values {
                    for axis in 0..3 {
                        minimum[axis] = minimum[axis].min(point[axis]);
                        maximum[axis] = maximum[axis].max(point[axis]);
                    }
                }
                if !values.is_empty() {
                    root["accessors"][accessor]["min"] = json!(minimum);
                    root["accessors"][accessor]["max"] = json!(maximum);
                }
            }
        }
        Ok(())
    }
}

impl TerrainPrimitive {
    fn rebuild_normals(&mut self) {
        self.normals.fill([0.0; 3]);
        for triangle in self.indices.chunks_exact(3) {
            let a = self.positions[triangle[0]];
            let b = self.positions[triangle[1]];
            let c = self.positions[triangle[2]];
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            for &index in triangle {
                for axis in 0..3 {
                    self.normals[index][axis] += n[axis];
                }
            }
        }
        for normal in &mut self.normals {
            let length = normal.iter().map(|value| value * value).sum::<f32>().sqrt();
            if length > f32::EPSILON {
                for value in normal {
                    *value /= length;
                }
            } else {
                *normal = [0.0, 1.0, 0.0];
            }
        }
    }
}

struct Layout {
    start: usize,
    stride: usize,
    count: usize,
}

fn accessor_layout(
    root: &Value,
    index: usize,
    bin_len: usize,
    component: u64,
    kind: &str,
    size: usize,
) -> Result<Layout> {
    let accessor = root
        .get("accessors")
        .and_then(|v| v.get(index))
        .context("missing terrain accessor")?;
    ensure!(
        accessor["componentType"] == component
            && accessor["type"] == kind
            && accessor.get("sparse").is_none(),
        "unsupported terrain accessor {index}: expected non-sparse {kind}/{component}"
    );
    let view_index = accessor["bufferView"]
        .as_u64()
        .context("terrain accessor has no bufferView")? as usize;
    let view = root
        .get("bufferViews")
        .and_then(|v| v.get(view_index))
        .context("missing terrain bufferView")?;
    ensure!(
        view["buffer"].as_u64().unwrap_or(0) == 0,
        "terrain uses an external buffer"
    );
    let view_start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let view_len = view["byteLength"]
        .as_u64()
        .context("terrain view has no length")? as usize;
    let start = view_start
        .checked_add(accessor["byteOffset"].as_u64().unwrap_or(0) as usize)
        .context("terrain offset overflow")?;
    let stride = view["byteStride"].as_u64().unwrap_or(size as u64) as usize;
    let count = accessor["count"]
        .as_u64()
        .context("terrain accessor has no count")? as usize;
    ensure!(stride >= size, "invalid terrain stride");
    let end = if count == 0 {
        start
    } else {
        start
            .checked_add(
                (count - 1)
                    .checked_mul(stride)
                    .context("terrain count overflow")?,
            )
            .and_then(|value| value.checked_add(size))
            .context("terrain range overflow")?
    };
    ensure!(
        end <= bin_len
            && end
                <= view_start
                    .checked_add(view_len)
                    .context("terrain view overflow")?,
        "terrain accessor is outside its buffer"
    );
    Ok(Layout {
        start,
        stride,
        count,
    })
}

fn read_vec3(root: &Value, bin: &[u8], accessor: usize) -> Result<Vec<[f32; 3]>> {
    let layout = accessor_layout(root, accessor, bin.len(), 5126, "VEC3", 12)?;
    let mut values = Vec::with_capacity(layout.count);
    for index in 0..layout.count {
        let point = std::array::from_fn(|axis| {
            let offset = layout.start + index * layout.stride + axis * 4;
            f32::from_le_bytes(bin[offset..offset + 4].try_into().unwrap())
        });
        ensure!(
            point.iter().all(|value| value.is_finite()),
            "terrain has non-finite coordinates"
        );
        values.push(point);
    }
    Ok(values)
}

fn read_terrain(root: &Value, bin: &[u8]) -> Result<Vec<TerrainPrimitive>> {
    let nodes = root["nodes"].as_array().context("map has no nodes")?;
    let node = nodes
        .iter()
        .find(|node| node["name"] == "terrain")
        .context("map has no terrain node")?;
    ensure!(
        ["matrix", "translation", "rotation", "scale"]
            .iter()
            .all(|key| node.get(*key).is_none()),
        "sculpting transformed terrain nodes is not supported"
    );
    let mesh_index = node["mesh"].as_u64().context("terrain node has no mesh")? as usize;
    let primitives = root["meshes"][mesh_index]["primitives"]
        .as_array()
        .context("terrain mesh has no primitives")?;
    let mut terrain = Vec::new();
    for (primitive_index, primitive) in primitives.iter().enumerate() {
        ensure!(
            primitive["mode"].as_u64().unwrap_or(4) == 4 && primitive.get("targets").is_none(),
            "unsupported terrain topology"
        );
        let position_accessor = primitive["attributes"]["POSITION"]
            .as_u64()
            .context("terrain has no positions")? as usize;
        let normal_accessor = primitive["attributes"]["NORMAL"]
            .as_u64()
            .context("terrain has no normals")? as usize;
        let positions = read_vec3(root, bin, position_accessor)?;
        let normals = read_vec3(root, bin, normal_accessor)?;
        ensure!(
            positions.len() == normals.len(),
            "terrain normal count mismatch"
        );
        let indices = if let Some(accessor) = primitive["indices"].as_u64() {
            let component = root["accessors"][accessor as usize]["componentType"]
                .as_u64()
                .context("missing index type")?;
            let size = match component {
                5121 => 1,
                5123 => 2,
                5125 => 4,
                _ => anyhow::bail!("unsupported terrain indices"),
            };
            let layout = accessor_layout(
                root,
                accessor as usize,
                bin.len(),
                component,
                "SCALAR",
                size,
            )?;
            (0..layout.count)
                .map(|index| {
                    let start = layout.start + index * layout.stride;
                    bin[start..start + size]
                        .iter()
                        .enumerate()
                        .fold(0usize, |value, (byte, part)| {
                            value | ((*part as usize) << (byte * 8))
                        })
                })
                .collect::<Vec<_>>()
        } else {
            (0..positions.len()).collect()
        };
        ensure!(
            indices.len().is_multiple_of(3) && indices.iter().all(|&index| index < positions.len()),
            "invalid terrain triangle indices"
        );
        terrain.push(TerrainPrimitive {
            mesh_index,
            primitive_index,
            positions,
            normals,
            indices,
            position_accessor,
            normal_accessor,
        });
    }
    Ok(terrain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain_document() -> (tempfile::TempDir, MapDocument) {
        let directory =
            super::super::tests::write_document(super::super::tests::root(lif::FORMAT_VERSION));
        let mut document = MapDocument::open(directory.path().join("map.glb")).unwrap();
        let mut accessors = Vec::new();
        for (index, values) in [
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, -10.0]],
            vec![[0.0, 1.0, 0.0]; 3],
        ]
        .iter()
        .enumerate()
        {
            while !document.bin_chunk.len().is_multiple_of(4) {
                document.bin_chunk.push(0);
            }
            let start = document.bin_chunk.len();
            for point in values {
                for value in point {
                    document
                        .bin_chunk
                        .extend_from_slice(&f32::to_le_bytes(*value));
                }
            }
            document.root["bufferViews"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "buffer": 0, "byteOffset": start, "byteLength": values.len() * 12
                }));
            accessors.push(json!({
                "bufferView": index + 1, "componentType": 5126, "type": "VEC3", "count": 3
            }));
        }
        document.root["accessors"] = json!(accessors);
        document.root["meshes"] =
            json!([{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}]);
        document.root["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "terrain", "mesh": 0}));
        document.json_chunk = serde_json::to_vec(&document.root).unwrap();
        (directory, document)
    }

    #[test]
    fn sculpt_updates_positions_normals_and_bounds_and_round_trips() {
        let (_directory, mut document) = terrain_document();
        let collision = document.collision_cells.clone();
        let original_bin = document.bin_chunk.clone();
        assert_eq!(document.sculpt_terrain(0, 0, 0, 2.0).unwrap(), 1);
        let terrain = document.terrain_primitives().unwrap();
        assert_eq!(
            terrain[0].positions,
            [[0.0, 2.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, -10.0]]
        );
        assert_ne!(terrain[0].normals[0], [0.0, 1.0, 0.0]);
        assert_eq!(
            document.collision_cells, collision,
            "sculpting must not change collision heights"
        );
        assert_eq!(
            document.bin_chunk, original_bin,
            "editing does not write serialized buffers"
        );
        assert!(document.undo());
        assert_eq!(
            document.terrain_primitives().unwrap()[0].positions[0][1],
            0.0
        );
        assert_eq!(
            document.terrain_primitives().unwrap()[0].normals[0],
            [0.0, 1.0, 0.0]
        );
        assert!(document.redo());
        document.save().unwrap();
        let reopened = MapDocument::open(&document.source_path).unwrap();
        let terrain = read_terrain(&reopened.root, &reopened.bin_chunk).unwrap();
        assert_eq!(terrain[0].positions[0][1], 2.0);
        assert_eq!(
            reopened.root["accessors"][0]["max"],
            json!([10.0, 2.0, 0.0])
        );
        assert_eq!(reopened.collision_cells, collision);
        let range = collision.len() * lif::LifCollision::BYTES_PER_CELL;
        assert_eq!(&original_bin[..range], &reopened.bin_chunk[..range]);
    }

    #[test]
    fn sculpt_rejects_unsupported_or_invalid_geometry_without_history() {
        let (_directory, mut document) = terrain_document();
        document.root["accessors"][0]["count"] = json!(999999);
        assert!(document.sculpt_terrain(0, 0, 0, 1.0).is_err());
        assert!(!document.is_dirty());
        assert!(!document.undo());
        assert!(document.terrain.is_none());
    }

    #[test]
    fn shared_vertices_across_materials_move_together_and_save_failure_keeps_edits() {
        let (directory, mut document) = terrain_document();
        let primitive = document.root["meshes"][0]["primitives"][0].clone();
        document.root["meshes"][0]["primitives"]
            .as_array_mut()
            .unwrap()
            .push(primitive);
        document.sculpt_terrain(0, 0, 0, 3.0).unwrap();
        let terrain = document.terrain_primitives().unwrap();
        assert_eq!(terrain[0].positions[0], terrain[1].positions[0]);
        assert_eq!(terrain[0].positions[0][1], 3.0);
        let bin = document.bin_chunk.clone();
        document.source_path = directory.path().join("missing/map.glb");
        assert!(document.save().is_err());
        assert_eq!(document.bin_chunk, bin);
        assert!(document.is_dirty());
        assert_eq!(
            document.terrain_primitives().unwrap()[0].positions[0][1],
            3.0
        );
        assert!(document.undo());
        assert_eq!(
            document.terrain_primitives().unwrap()[0].positions[0][1],
            0.0
        );
    }
}
