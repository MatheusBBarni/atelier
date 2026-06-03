pub type Id = String;

pub fn new_id() -> Id {
    ulid::Ulid::new().to_string()
}
