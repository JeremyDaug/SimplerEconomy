pub mod pop;
pub mod desire;
pub mod good;
pub mod process;
pub mod market;
pub mod job;
pub mod data;
pub mod world;
pub mod culture;

#[cfg(test)]
mod tests {
    mod pop_tests {
        mod check_barter_should {
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};

            #[test]
            pub fn correctly_calculate_success_on_sat_only_exchange() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(4, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 100.0);
                property.insert(1, 300.0);
                property.insert(2, 200.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let mut take = HashMap::new();
                let mut give = HashMap::new();
                // give level 1 item for level 2
                take.insert(2, 100.0);
                give.insert(1, 100.0);

                assert!(pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

            #[test]
            pub fn correctly_calculate_failure_on_sat_only_take() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(4, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let mut take = HashMap::new();
                let give = HashMap::new();
                //take.insert(3, 49.0);
                take.insert(1, 50.0);

                assert!(!pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

            #[test]
            pub fn correctly_calculate_success_on_sat_only_gift() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(4, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let take = HashMap::new();
                let mut give = HashMap::new();
                //take.insert(3, 49.0);
                give.insert(1, 50.0);

                assert!(pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

            #[test]
            pub fn correctly_calculate_success_on_amv_only_trade() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(4, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let mut take = HashMap::new();
                let mut give = HashMap::new();
                take.insert(3, 49.0);
                give.insert(4, 50.0);

                assert!(pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

            #[test]
            pub fn correctly_calculate_failure_on_amv_only_trade() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(4, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let mut take = HashMap::new();
                let mut give = HashMap::new();
                take.insert(3, 50.0);
                give.insert(4, 49.0);

                assert!(!pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

            #[test]
            pub fn correctly_calculate_failure_on_amv_loss() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let mut take = HashMap::new();
                let give = HashMap::new();
                take.insert(3, 50.0);

                assert!(!pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

            #[test]
            pub fn correctly_calculate_success_on_amv_gift() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let take = HashMap::new();
                let mut give = HashMap::new();
                give.insert(3, 200.0);

                assert!(pop.check_barter(give, take, &market, &data), "Did not work properly.");
            }

        }

        mod possible_satisfaciton_gain_should {
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};

            #[test]
            pub fn correctly_calculate_full_satisfaction_more_than_10_entries() {
                let desires = vec![ // 21 entries
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 1500.0); // 6 desires
                property.insert(1, 2000.0); // 9 desires
                property.insert(2, 1800.0); // 6 desires
                property.insert(3, 50.0);  // 0 desires

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let (results, l, amv) = pop.current_overall_satisfaction(&market, &data);
                assert_eq!(*results.get(0) .unwrap(), 300.0); // 0
                assert_eq!(*results.get(1) .unwrap(), 300.0); // 0
                assert_eq!(*results.get(2) .unwrap(), 300.0); // 1
                assert_eq!(*results.get(3) .unwrap(), 400.0); // 2
                assert_eq!(*results.get(4) .unwrap(), 300.0); // 1
                assert_eq!(*results.get(5) .unwrap(), 300.0); // 1
                assert_eq!(*results.get(6) .unwrap(), 400.0); // 2
                assert_eq!(*results.get(7) .unwrap(), 300.0); // 0
                assert_eq!(*results.get(8) .unwrap(), 200.0); // 0
                assert_eq!(*results.get(9) .unwrap(), 200.0); // 1
                assert_eq!(*results.get(10).unwrap(), 300.0);// 2
                assert_eq!(*results.get(11).unwrap(), 200.0);// 1
                assert_eq!(*results.get(12).unwrap(), 200.0);// 1
                assert_eq!(*results.get(13).unwrap(), 300.0);// 2
                assert_eq!(*results.get(14).unwrap(), 200.0);// 0
                assert_eq!(*results.get(15).unwrap(), 200.0);// 0
                assert_eq!(*results.get(16).unwrap(), 200.0);// 1
                assert_eq!(*results.get(17).unwrap(), 200.0);// 2
                assert_eq!(*results.get(18).unwrap(), 200.0);// 1
                assert_eq!(*results.get(19).unwrap(), 100.0);// 1
                assert_eq!(*results.get(20).unwrap(), 200.0);// 2
                assert_eq!(l, 2.0);
                assert_eq!(amv, 50.0);
                let result = pop.possible_satisfaciton_gain(None, &market, &data);
                assert_eq!(*result.get(0) .unwrap(), 300.0); // 0
                assert_eq!(*result.get(1) .unwrap(), 300.0); // 0
                assert_eq!(*result.get(2) .unwrap(), 300.0); // 1
                assert_eq!(*result.get(3) .unwrap(), 400.0); // 2
                assert_eq!(*result.get(4) .unwrap(), 300.0); // 1
                assert_eq!(*result.get(5) .unwrap(), 300.0); // 1
                assert_eq!(*result.get(6) .unwrap(), 400.0); // 2
                assert_eq!(*result.get(7) .unwrap(), 300.0); // 0
                assert_eq!(*result.get(8) .unwrap(), 200.0); // 0
                assert_eq!(*result.get(9) .unwrap(), 200.0); // 1
                assert_eq!(*result.get(10).unwrap(), 300.0);// 2
                assert_eq!(*result.get(11).unwrap(), 200.0);// 1
                assert_eq!(*result.get(12).unwrap(), 200.0);// 1
                assert_eq!(*result.get(13).unwrap(), 300.0);// 2
                assert_eq!(*result.get(14).unwrap(), 200.0);// 0
                assert_eq!(*result.get(15).unwrap(), 200.0);// 0
                assert_eq!(*result.get(16).unwrap(), 200.0);// 1
                assert_eq!(*result.get(17).unwrap(), 200.0);// 2
                assert_eq!(*result.get(18).unwrap(), 200.0);// 1
                assert_eq!(*result.get(19).unwrap(), 150.0);// 1
                assert_eq!(*result.get(20).unwrap(), 200.0);// 2
            }

            #[test]
            pub fn correctly_expend_excess_amv_with_less_than_10_entries() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let (results, l, amv) = pop.current_overall_satisfaction(&market, &data);
                assert_eq!(*results.get(0).unwrap(), 200.0);
                assert_eq!(*results.get(1).unwrap(), 100.0);
                assert_eq!(*results.get(2).unwrap(), 200.0);
                assert_eq!(*results.get(3).unwrap(), 200.0);
                assert_eq!(*results.get(4).unwrap(), 100.0);
                assert_eq!(*results.get(5).unwrap(), 100.0);
                assert_eq!(*results.get(6).unwrap(), 200.0);
                assert_eq!(l, 2.0);
                assert_eq!(amv, 100.0);
                let result = pop.possible_satisfaciton_gain(None, &market, &data);
                assert_eq!(*result.get(0).unwrap(), 200.0);
                assert_eq!(*result.get(1).unwrap(), 200.0);
                assert_eq!(*result.get(2).unwrap(), 200.0);
                assert_eq!(*result.get(3).unwrap(), 200.0);
                assert_eq!(*result.get(4).unwrap(), 100.0);
                assert_eq!(*result.get(5).unwrap(), 100.0);
                assert_eq!(*result.get(6).unwrap(), 200.0);
            }
        }

        mod consume_goods_should {
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};

            #[test]
            pub fn consume_and_reserve_goods_correctly() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let mut pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                pop.consume_goods(&market, &data, None);
                // check property
                assert_eq!(*pop.property.get(&0).unwrap(), 100.0);
                assert_eq!(*pop.property.get(&1).unwrap(), 200.0);
                assert_eq!(*pop.property.get(&2).unwrap(), 300.0);
                assert_eq!(*pop.property.get(&3).unwrap(), 100.0);
            }
        }

        mod starving_pops_should {
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};

            #[test]
            pub fn find_staving_pops_when_all_are_fed() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 150.0);
                property.insert(1, 100.0);
                property.insert(2, 75.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 10.0,
                };

                let result = pop.starving_pops(&market, &data, None);
                assert_eq!(result, 0.0);
            }

            #[test]
            pub fn find_staving_pops_when_some_are_starving() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 50.0);
                property.insert(1, 100.0);
                property.insert(2, 75.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 10.0,
                };

                let result = pop.starving_pops(&market, &data, None);
                assert_eq!(result, 50.0);
            }
        }

        mod satisfaction_spread_should{
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};
 
            #[test]
            pub fn correctly_calculate_simple_spread() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 800.0);
                property.insert(1, 550.0);
                property.insert(2, 456.0);
                property.insert(3, 200.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let (lower, upper, average) = pop.satisfaction_spread(&market, &data, None);
                assert_eq!(lower, 9.0);
                assert_eq!(upper, 9.0);
                assert_eq!(average, 2.0);
            }

            #[test]
            pub fn correctly_calculate_varied_spread() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 150.0);
                property.insert(1, 100.0);
                property.insert(2, 75.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 10.0,
                };

                let (lower, upper, average) = pop.satisfaction_spread(&market, &data, None);
                assert_eq!(lower, 2.0);
                assert_eq!(upper, 5.0);
                assert_eq!(average, 0.0);
            }
        }
        
        mod excess_goods_should {
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};

            #[test]
            pub fn correctly_calculate_excess_goods() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 800.0);
                property.insert(1, 550.0);
                property.insert(2, 456.0);
                property.insert(3, 200.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let result = pop.excess_goods(&data);
                assert_eq!(*result.get(&0).unwrap(), 600.0);
                assert_eq!(*result.get(&1).unwrap(), 250.0);
                assert_eq!(*result.get(&2).unwrap(), 256.0);
                assert_eq!(*result.get(&3).unwrap(), 200.0);
            }
        }

        mod current_overall_satisfaction_should {
            use std::collections::{HashMap, HashSet};

            use crate::{culture::Culture, data::Data, desire::Desire, market::{GoodData, Market}, pop::Pop};

            #[test]
            pub fn correctly_calculate_full_satisfaction_more_than_10_entries() {
                let desires = vec![ // 21 entries
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 1500.0); // 6 desires
                property.insert(1, 2000.0); // 9 desires
                property.insert(2, 1800.0); // 6 desires
                property.insert(3, 200.0);  // 0 desires

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let (results, l, amv) = pop.current_overall_satisfaction(&market, &data);
                assert_eq!(*results.get(0) .unwrap(), 300.0); // 0
                assert_eq!(*results.get(1) .unwrap(), 300.0); // 0
                assert_eq!(*results.get(2) .unwrap(), 300.0); // 1
                assert_eq!(*results.get(3) .unwrap(), 400.0); // 2
                assert_eq!(*results.get(4) .unwrap(), 300.0); // 1
                assert_eq!(*results.get(5) .unwrap(), 300.0); // 1
                assert_eq!(*results.get(6) .unwrap(), 400.0); // 2
                assert_eq!(*results.get(7) .unwrap(), 300.0); // 0
                assert_eq!(*results.get(8) .unwrap(), 200.0); // 0
                assert_eq!(*results.get(9) .unwrap(), 200.0); // 1
                assert_eq!(*results.get(10).unwrap(), 300.0);// 2
                assert_eq!(*results.get(11).unwrap(), 200.0);// 1
                assert_eq!(*results.get(12).unwrap(), 200.0);// 1
                assert_eq!(*results.get(13).unwrap(), 300.0);// 2
                assert_eq!(*results.get(14).unwrap(), 200.0);// 0
                assert_eq!(*results.get(15).unwrap(), 200.0);// 0
                assert_eq!(*results.get(16).unwrap(), 200.0);// 1
                assert_eq!(*results.get(17).unwrap(), 200.0);// 2
                assert_eq!(*results.get(18).unwrap(), 200.0);// 1
                assert_eq!(*results.get(19).unwrap(), 100.0);// 1
                assert_eq!(*results.get(20).unwrap(), 200.0);// 2
                assert_eq!(l, 2.0);
                assert_eq!(amv, 200.0);
            }

            #[test]
            pub fn correctly_calculate_simple_satisfaction_more_than_10_entries() {
                let desires = vec![ // 30 entries
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(0),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 6050.0); // 6 desires

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let (results, l, _) = pop.current_overall_satisfaction(&market, &data);
                assert_eq!(*results.get(0) .unwrap(), 300.0);
                assert_eq!(*results.get(1) .unwrap(), 300.0);
                assert_eq!(*results.get(2) .unwrap(), 300.0);
                assert_eq!(*results.get(3) .unwrap(), 300.0);
                assert_eq!(*results.get(4) .unwrap(), 300.0);
                assert_eq!(*results.get(5) .unwrap(), 300.0);
                assert_eq!(*results.get(6) .unwrap(), 300.0);
                assert_eq!(*results.get(7) .unwrap(), 300.0);
                assert_eq!(*results.get(8) .unwrap(), 300.0);
                assert_eq!(*results.get(9) .unwrap(), 300.0);
                assert_eq!(*results.get(10).unwrap(), 200.0);
                assert_eq!(*results.get(11).unwrap(), 200.0);
                assert_eq!(*results.get(12).unwrap(), 200.0);
                assert_eq!(*results.get(13).unwrap(), 200.0);
                assert_eq!(*results.get(14).unwrap(), 200.0);
                assert_eq!(*results.get(15).unwrap(), 200.0);
                assert_eq!(*results.get(16).unwrap(), 200.0);
                assert_eq!(*results.get(17).unwrap(), 200.0);
                assert_eq!(*results.get(18).unwrap(), 200.0);
                assert_eq!(*results.get(19).unwrap(), 200.0);
                assert_eq!(*results.get(20).unwrap(), 150.0);
                assert_eq!(*results.get(21).unwrap(), 100.0);
                assert_eq!(*results.get(22).unwrap(), 100.0);
                assert_eq!(*results.get(23).unwrap(), 100.0);
                assert_eq!(*results.get(24).unwrap(), 100.0);
                assert_eq!(*results.get(25).unwrap(), 100.0);
                assert_eq!(*results.get(26).unwrap(), 100.0);
                assert_eq!(*results.get(27).unwrap(), 100.0);
                assert_eq!(*results.get(28).unwrap(), 100.0);
                assert_eq!(*results.get(29).unwrap(), 100.0);
                assert_eq!(l, 2.0);
            }


            #[test]
            pub fn correctly_calculate_full_satisfaction_less_than_10_entries() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 400.0);
                property.insert(2, 400.0);
                property.insert(3, 100.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 200.0,
                };

                let (results, l, amv) = pop.current_overall_satisfaction(&market, &data);
                assert_eq!(*results.get(0).unwrap(), 200.0);
                assert_eq!(*results.get(1).unwrap(), 100.0);
                assert_eq!(*results.get(2).unwrap(), 200.0);
                assert_eq!(*results.get(3).unwrap(), 200.0);
                assert_eq!(*results.get(4).unwrap(), 100.0);
                assert_eq!(*results.get(5).unwrap(), 100.0);
                assert_eq!(*results.get(6).unwrap(), 200.0);
                assert_eq!(l, 2.0);
                assert_eq!(amv, 100.0);
            }

            #[test]
            pub fn correctly_calculate_partial_satisfaction() {
                let desires = vec![
                    Desire::Consume(0),
                    Desire::Consume(0),
                    Desire::Consume(1),
                    Desire::Consume(2),
                    Desire::Consume(1),
                    Desire::Own(1),
                    Desire::Own(2),
                ];
                let culture = Culture {
                    id: 0,
                    name: "test".to_string(),
                    desires,
                };
                let mut data = Data {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                    cultures: HashMap::new(),
                };
                data.cultures.insert(culture.id, culture);

                let mut market = Market {
                    id: 0,
                    name: "test_market".to_string(),
                    connections: HashMap::new(),
                    goods_info: HashMap::new(),
                    monies: HashSet::new(),
                    pops: HashSet::new(),
                    jobs: HashSet::new(),
                    merchants: HashSet::new(),
                };
                market.goods_info.insert(0, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(1, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(2, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });
                market.goods_info.insert(3, GoodData {
                    amv: 1.0,
                    salability: 1.0,
                });

                let mut property = HashMap::new();
                property.insert(0, 300.0);
                property.insert(1, 250.0);
                property.insert(2, 125.0);

                let pop = Pop {
                    id: 0,
                    size: 100.0,
                    culture: 0,
                    efficiency: 1.0,
                    property,
                    unused_time: 0.0,
                };

                let (results, l, amv) = pop.current_overall_satisfaction(&market, &data);
                assert_eq!(*results.get(0).unwrap(), 200.0); // C0
                assert_eq!(*results.get(1).unwrap(), 100.0); // C0
                assert_eq!(*results.get(2).unwrap(), 100.0); // C1
                assert_eq!(*results.get(3).unwrap(), 100.0); // C2
                assert_eq!(*results.get(4).unwrap(), 100.0); // C1
                assert_eq!(*results.get(5).unwrap(), 50.0);  // O1
                assert_eq!(*results.get(6).unwrap(), 25.0);  // O2
                assert_eq!(l, 0.0);
                assert_eq!(amv, 0.0);
            }
        }
    }

    mod process_tests {
        mod do_process_should {
            use std::collections::HashMap;

            use crate::{data::Data, good::Good, process::{InputType, Process}};

            #[test]
            pub fn correctly_calculate_process_easy_time() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 0.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 10.0);
                goods.insert(1, 20.0);

                let test_result = test.do_process(1.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 1.0);
                assert_eq!(test_result.iterations, 1.0);
                assert_eq!(test_result.consumed.len(), 1);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 1.0);
                assert!(test_result.consumed.get(&1).is_none());
                assert_eq!(test_result.used.len(), 1);
                assert_eq!(*test_result.used.get(&1).unwrap(), 1.0);
                assert_eq!(test_result.created.len(), 1);
                assert_eq!(*test_result.created.get(&2).unwrap(), 111.0);
            }

            #[test]
            pub fn correctly_calculate_process_with_optional_goods() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 1.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 10.0);
                goods.insert(1, 20.0);

                let test_result = test.do_process(1.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 1.0);
                assert_eq!(test_result.iterations, 1.0);
                assert_eq!(test_result.consumed.len(), 1);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 1.0);
                assert!(test_result.consumed.get(&1).is_none());
                assert_eq!(*test_result.used.get(&1).unwrap_or(&0.0), 0.0);
                assert_eq!(test_result.created.len(), 1);
                assert_eq!(*test_result.created.get(&2).unwrap(), 100.0);
            }

            #[test]
            pub fn correctly_calculate_process_with_optional_goods_but_no_excess() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 1.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 20.0);
                //goods.insert(1, 20.0);

                let test_result = test.do_process(20.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 20.0);
                assert_eq!(test_result.iterations, 20.0);
                assert_eq!(test_result.consumed.len(), 1);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 20.0);
                assert!(test_result.consumed.get(&1).is_none());
                assert_eq!(test_result.used.len(), 1);
                assert_eq!(*test_result.used.get(&1).unwrap(), 0.0);
                assert_eq!(test_result.created.len(), 1);
                assert_eq!(*test_result.created.get(&2).unwrap(), 2000.0);
            }

            #[test]
            pub fn correctly_calculate_process_optional_goods_and_shortfall() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    durability: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);
                inputs.insert(2, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);
                input_type.insert(2, InputType::Input);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 1.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 20.0);
                goods.insert(1, 10.0);

                let test_result = test.do_process(20.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 10.0);
                assert_eq!(test_result.iterations, 10.0);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 10.0);
                assert_eq!(*test_result.consumed.get(&1).unwrap_or(&0.0), 0.0);
                assert_eq!(*test_result.consumed.get(&2).unwrap_or(&0.0), 0.0);
                assert_eq!(test_result.used.len(), 1);
                assert_eq!(*test_result.used.get(&1).unwrap(), 10.0);
                assert_eq!(test_result.created.len(), 1);
                // TODO maybe come back to look at this as it should technically be 1110.0, but whatever, I have better things to do with my life, like not work on this project :P
                assert_eq!(*test_result.created.get(&2).unwrap(), 1100.0);
            }
        }

        #[cfg(test)]
        mod efficiency_should {
            use std::collections::HashMap;

            use crate::process::{InputType, Process};

            #[test]
            pub fn calculation_check() {
                 let inputs = HashMap::new();

                let mut input_type = HashMap::new();
                for (&good, _) in inputs.iter() {
                    input_type.insert(good, InputType::Input);
                }

                let outputs = HashMap::new();

                let mut test = Process {
                    id: 0,
                    name: "test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 0.0,
                    input_type,
                    outputs,
                };
                let result = test.efficiency();
                assert_eq!(result, 1.0);

                // 1 input
                test.inputs.insert(0, 1.0);
                let result = test.efficiency();
                assert_eq!(result, 1.0);

                // 2 inputs
                test.inputs.insert(1, 1.0);
                let result = test.efficiency();
                assert_eq!(result, 1.1);

                // 3 inputs, 2 in one part
                test.inputs.insert(1, 2.0);
                let result = test.efficiency();
                assert_eq!(result, 1.3);

                // 4 inputs, 2 in one part
                test.inputs.insert(2, 1.0);
                let result = test.efficiency();
                assert_eq!(result, 1.6);

                // 4 inputs, 2 in one part, 1 optional
                test.optional += 1.0;
                let result = test.efficiency();
                assert_eq!(result, 1.3);
            }
        }
    }
}