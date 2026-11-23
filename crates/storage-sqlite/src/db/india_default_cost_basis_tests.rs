use super::DbAccess;
use rusqlite::Connection;

const UP: &str = include_str!("../../migrations/2026-11-23-000001_india_default_cost_basis/up.sql");

fn method(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT json_extract(meta, '$.accounting.costBasisMethod') FROM accounts WHERE id = ?1",
        [id],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn moves_inr_accounts_on_the_old_default_to_same_day_fifo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.db");
    DbAccess::plaintext(path.to_str().unwrap())
        .run_migrations()
        .unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        r#"INSERT INTO accounts (id, name, currency, meta) VALUES
            ('inr-fifo', 'A', 'INR', '{"accounting":{"costBasisMethod":"FIFO","poolingScope":"ACCOUNT"},"x":1}'),
            ('inr-snake', 'B', 'INR', '{"accounting":{"cost_basis_method":"FIFO"}}'),
            ('inr-none', 'C', 'INR', NULL),
            ('inr-empty', 'D', 'INR', '{"x":2}'),
            ('inr-wac', 'E', 'INR', '{"accounting":{"costBasisMethod":"WAC"}}'),
            ('inr-bad', 'F', 'INR', 'not json'),
            ('usd-fifo', 'G', 'USD', '{"accounting":{"costBasisMethod":"FIFO"}}');
         DELETE FROM projection_state;"#,
    )
    .unwrap();
    conn.execute_batch(UP).unwrap();

    for id in ["inr-fifo", "inr-snake", "inr-none", "inr-empty"] {
        assert_eq!(method(&conn, id).as_deref(), Some("FIFO_SAME_DAY"), "{id}");
    }
    assert_eq!(method(&conn, "inr-wac").as_deref(), Some("WAC"));
    assert_eq!(method(&conn, "usd-fifo").as_deref(), Some("FIFO"));
    let bad: String = conn
        .query_row("SELECT meta FROM accounts WHERE id = 'inr-bad'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(bad, "not json");
    let kept: (i64, String, Option<String>) = conn
        .query_row(
            "SELECT json_extract(meta, '$.x'), json_extract(meta, '$.accounting.poolingScope'),
                    json_extract(meta, '$.accounting.cost_basis_method')
             FROM accounts WHERE id = 'inr-fifo'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(kept, (1, "ACCOUNT".to_string(), None));
    let snake: Option<String> = conn
        .query_row(
            "SELECT json_extract(meta, '$.accounting.cost_basis_method') FROM accounts WHERE id = 'inr-snake'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(snake, None);

    // The accounts trigger marks each changed account for recalculation.
    let marked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM projection_state WHERE dirty_from IS NOT NULL
               AND scope IN ('inr-fifo', 'inr-snake', 'inr-empty', 'inr-none', 'inr-wac', 'usd-fifo')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(marked, 4);
}
