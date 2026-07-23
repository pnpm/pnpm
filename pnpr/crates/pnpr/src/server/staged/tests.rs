use super::generate_stage_id;

#[test]
fn generate_stage_id_produces_hyphenated_v4_uuids() {
    let id = generate_stage_id();
    assert_eq!(id.len(), 36);
    let dash_positions: Vec<usize> =
        id.char_indices().filter(|(_, char)| *char == '-').map(|(index, _)| index).collect();
    assert_eq!(dash_positions, [8, 13, 18, 23]);
    assert_eq!(id.as_bytes()[14], b'4', "version nibble must be 4: {id}");
    assert_ne!(generate_stage_id(), id, "two ids must not collide");
}
