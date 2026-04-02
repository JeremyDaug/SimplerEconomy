use std::iter::Map;

use bevy::{
    asset::RenderAssetUsages, color::palettes::css::WHITE, mesh::{Indices, PrimitiveTopology}, platform::collections::HashMap, prelude::* 
};
use hexx::{
    Hex, 
    HexLayout, 
    MeshInfo, 
    PlaneMeshBuilder, 
    shapes::{
        self, 
        Parallelogram, 
        hexagon, 
        parallelogram
    }
};

const HEX_SIZE: f32 = 12.0;

#[derive(Debug, Resource)]
struct HexMap {
    layout: HexLayout,
    entities: HashMap<Hex, Entity>,
    default_material: Handle<ColorMaterial>,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Hexonomy".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, (setup_camera, setup_grid))
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn setup_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Hex Layout configuration
    let layout = HexLayout::pointy().with_hex_size(HEX_SIZE);
    // default material (for testing purposes)
    let default_material = materials.add(Color::Srgba(WHITE));
    // mesh
    let mesh = hexagonal_plane(&layout);
    let mesh_handle = meshes.add(mesh);

    let entities = shapes::parallelogram(Hex::new(0,0), Hex::new(20,30))
        .map(|hex| {
            let pos = layout.hex_to_world_pos(hex);
            let id = commands
                .spawn((
                    Mesh2d(mesh_handle.clone()),
                    MeshMaterial2d(default_material.clone()),
                    Transform::from_xyz(pos.x, pos.y, 0.0),
                    children![(
                        Text2d(format!("{},{}", hex.x, hex.y)),
                        TextColor(Color::BLACK),
                        TextFont {
                            font_size: 6.0,
                            ..default()
                        },
                        Transform::from_xyz(0.0, 0.0, 10.0),
                    )],
                ))
                .id();
            (hex, id)
        })
        .collect();

    commands.insert_resource(HexMap {
        layout,
        entities,
        default_material
    });
}

/// Compute a bevy mesh from the layout
fn hexagonal_plane(hex_layout: &HexLayout) -> Mesh {
    let mesh_info = PlaneMeshBuilder::new(hex_layout)
        .facing(Vec3::Z)
        .with_scale(Vec3::splat(0.98))
        .center_aligned()
        .build();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, mesh_info.vertices)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, mesh_info.normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, mesh_info.uvs)
    .with_inserted_indices(Indices::U16(mesh_info.indices))
}