use ecs_builder::EcsCode;
use std::io::BufReader;

#[test]
fn it_works() {
    let file = include_str!("ecs.yaml");
    let reader = BufReader::new(file.as_bytes());
    EcsCode::generate(reader).expect("Failed to build ECS");
}
