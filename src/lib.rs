pub mod game;

#[cfg(test)]
mod test {
    mod map {
        mod wrap {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let testmap = Map::new(10, 10);

                let underrow = Hex::new(6, -4);
                let overrow = Hex::new(5, 15);
                let undercol = Hex::new(-7, 3);
                let overcol = Hex::new(12, 2);

                // no wrapping
                assert!(testmap.wrap(underrow).is_none());
                assert!(testmap.wrap(overrow).is_none());
                assert!(testmap.wrap(undercol).is_none());
                assert!(testmap.wrap(overcol).is_none());

                // full wrapping
                let testmap = testmap.with_hwrap().with_vwrap();

                let undershoot = Hex::new(-4, -3);
                let overshoot = Hex::new(15, 13);

                if let Some(underres) = testmap.wrap(undershoot) {
                    assert_eq!(underres.x, 6);
                    assert_eq!(underres.y, 7);
                } else { assert!(false) }

                if let Some(overres) = testmap.wrap(overshoot) {
                    assert_eq!(overres.x, 5);
                    assert_eq!(overres.y, 3);
                } else { assert!(false) }
            }
        }

        mod get_region {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);
                testmap.tiles[1][1].region = Some(0);

                // check one from each grouping. Row zero should return none.
                assert_eq!(testmap.get_region(Hex {x:0, y: 0}), None);
                assert_eq!(testmap.get_region(Hex {x:1, y: 1}), Some(0));
            }
        }

        mod find_city_hex {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                testmap.regions.push(vec![Hex {x: 2, y: 2}]);
                testmap.tiles[2][2].region = Some(0);
                testmap.regions[0].push(Hex {x: 1, y: 2});
                testmap.tiles[1][2].region = Some(0);
                
                testmap.regions.push(vec![Hex {x: 4, y: 4}]);
                testmap.tiles[4][4].region = Some(1);
                testmap.regions[1].push(Hex {x: 4, y: 3});
                testmap.tiles[4][3].region = Some(1);
                
                testmap.regions.push(vec![Hex {x: 3, y: 1}]);
                testmap.tiles[3][1].region = Some(2);

                // check both exact find, secondary, and failed find
                if let Some(res) = testmap.find_city_hex(Hex { x: 2, y: 2 }) {
                    assert_eq!(res, Hex { x: 2, y: 2 });
                } else { assert!(false); }

                if let Some(res) = testmap.find_city_hex(Hex { x: 4, y: 3 }) {
                    assert_eq!(res, Hex { x: 4, y: 4 });
                } else { assert!(false); }

                assert!(testmap.find_city_hex(Hex {x: 0, y: 0}).is_none());
            }
        }

        mod get_city_hexes {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);
                testmap.regions.push(vec![Hex {x: 2, y: 2}]);
                testmap.regions[0].push(Hex {x: 1, y: 2});
                
                testmap.regions.push(vec![Hex {x: 4, y: 4}]);
                testmap.regions[1].push(Hex {x: 4, y: 3});
                
                testmap.regions.push(vec![Hex {x: 5, y: 1}]);

                // check one from each grouping. Row zero should return none.
                let res = testmap.get_city_hexes();
                assert_eq!(res.len(), 3);
                assert_eq!(res[0], Hex {x: 2, y: 2});
                assert_eq!(res[1], Hex {x: 4, y: 4});
                assert_eq!(res[2], Hex {x: 5, y: 1});
            }
        }

        mod get_region_neighbors {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                // set up a test region of size 1.
                testmap.regions.push(vec![Hex { x: 3, y: 3}]);
                // check that all neighbors are there by removing matches.
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 6);
                assert!(neighbors.contains(&Hex { x: 2, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));
                // check larger setup.
                testmap.regions[0].push(Hex { x: 2, y: 3 });
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 8);
                assert!(neighbors.contains(&Hex { x: 4, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));

                assert!(neighbors.contains(&Hex { x: 1, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 1, y: 4 }));
                // expand to an edge which doesn't wrap
                testmap.regions[0].push(Hex { x: 4, y: 3 });
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 8);
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));

                assert!(neighbors.contains(&Hex { x: 1, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 1, y: 4 }));

                assert!(neighbors.contains(&Hex { x: 4, y: 4 }));
                // let wrapping occur
                testmap.hwrap = true;
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 10);
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));

                assert!(neighbors.contains(&Hex { x: 1, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 1, y: 4 }));

                assert!(neighbors.contains(&Hex { x: 4, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 0, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 0, y: 2 }));
            }
        }

        mod is_region_neighbor {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                // set up a test region.
                testmap.regions.push(vec![Hex { x: 3, y: 3}]);
                testmap.regions[0].push(Hex { x: 2, y: 3 });
                testmap.regions[0].push(Hex { x: 4, y: 3 });

                testmap.tiles[3][3].region = Some(0);
                testmap.tiles[2][3].region = Some(0);
                testmap.tiles[4][3].region = Some(0);

                // within region
                assert!(!testmap.is_region_neighbor(0, Hex { x: 3, y: 3 }));
                // out of bounds neighbor
                assert!(!testmap.is_region_neighbor(0, Hex { x: 5, y: 3 }));
                assert!(!testmap.is_region_neighbor(0, Hex { x: 10, y: 3 }));
                // wrapped neighbor
                testmap.hwrap = true;
                assert!(testmap.is_region_neighbor(0, Hex { x: 5, y: 3 }));
                assert!(testmap.is_region_neighbor(0, Hex { x: 10, y: 3 }));
                // neighbor
                assert!(testmap.is_region_neighbor(0, Hex { x: 4, y: 2 }));
                // not neighbor
                assert!(!testmap.is_region_neighbor(0, Hex { x: 1, y: 1 }));
            }
        }

        mod add_city {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(10, 10);

                // in bounds
                let city1 = Hex::new(3, 4);
                assert!(testmap.add_city(city1));
                assert_eq!(testmap.regions.len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().get(0).unwrap(), city1);

                // taken
                let city2 = Hex { x: 12, y: 14 };
                assert!(!testmap.add_city(city2));
                assert_eq!(testmap.regions.len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().get(0).unwrap(), city1);
                // out of bounds
                testmap.vwrap = true;
                testmap.hwrap = true;
                assert!(testmap.add_city(city2));
                assert_eq!(testmap.regions.len(), 2);
                assert_eq!(testmap.regions.get(0).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().get(0).unwrap(), city1);
                assert_eq!(testmap.regions.get(1).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(1).unwrap().get(0).unwrap(), Hex {x: 2, y: 4});
            }
        }
    
        mod add_to_region {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                // set up a test region.
                testmap.regions.push(vec![Hex { x: 3, y: 3}]);
                testmap.regions[0].push(Hex { x: 2, y: 3 });
                testmap.regions[0].push(Hex { x: 4, y: 3 });

                testmap.tiles[3][3].region = Some(0);
                testmap.tiles[2][3].region = Some(0);
                testmap.tiles[4][3].region = Some(0);

                testmap.regions.push(vec![Hex::new(1, 3)]);
                testmap.tiles[1][3].region = Some(1);

                // within region
                assert!(!testmap.add_to_region(0, Hex {x: 3, y: 3}));
                // in another region
                assert!(!testmap.add_to_region(0, Hex {x: 1, y: 3}));
                // not neighbor
                assert!(!testmap.add_to_region(0, Hex {x: 0, y: 0}));
                // out of bounds
                assert!(!testmap.add_to_region(0, Hex {x: 10, y: 10}));
                // wrapped neighbor
                testmap.hwrap = true;
                assert!(testmap.add_to_region(0, Hex {x: 8, y: 4}));
                assert_eq!(testmap.regions[0].len(), 4);
                assert_eq!(testmap.regions[0][3], Hex::new(3, 4));
                assert_eq!(testmap.tiles[3][4].region.unwrap(), 0);
                // normal neighbor
                assert!(testmap.add_to_region(0, Hex {x: 3, y: 2}));
                assert_eq!(testmap.regions[0].len(), 5);
                assert_eq!(testmap.regions[0][4], Hex::new(3, 2));
                assert_eq!(testmap.tiles[3][2].region.unwrap(), 0);
            }
        }
    }
}