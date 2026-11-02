use crate::is_transaction_conflict;
use libsql::Error as LibsqlError;

fn hrana(message: &str) -> LibsqlError {
    LibsqlError::Hrana(std::io::Error::other(message.to_string()).into())
}

#[test]
fn a_lock_reported_by_its_message_alone_is_a_conflict() {
    assert!(is_transaction_conflict(&hrana("SQLite failure: `database is locked`")));
    assert!(is_transaction_conflict(&hrana("SQLite failure: `database table is locked`")));
    assert!(!is_transaction_conflict(&hrana("SQLite failure: `UNIQUE constraint failed`")));
}
