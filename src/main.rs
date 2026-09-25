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
}
