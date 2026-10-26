use lightshade::prelude::*;
use lightshade::table::Value as NewValue;
use lightshade_codegen::ir::Value as OldValue;
use lightshade_codegen::ir::{parse_workload, validate};
use lightshade_codegen::runtime::{Database, OpResult};
use rust_suite::{
    catalog::{self, Product},
    events::{self, Event},
    inbox_digest::{self, Inbox, Msg},
    kv::{self, Kv},
    order_book::{self, Order},
    sessions::{self, Session},
};

fn legacy(source: &str) -> Database {
    let workload = parse_workload(source).unwrap();
    validate(&workload).unwrap();
    Database::new(workload)
}

fn norm_old(rows: Vec<Vec<OldValue>>) -> Vec<Vec<String>> {
    rows.into_iter()
        .map(|row| row.iter().map(|value| format!("{value:?}")).collect())
        .collect()
}

fn norm_new(rows: Vec<Vec<NewValue>>) -> Vec<Vec<String>> {
    rows.into_iter()
        .map(|row| row.iter().map(|value| format!("{value:?}")).collect())
        .collect()
}

fn many(result: OpResult) -> Vec<Vec<OldValue>> {
    match result {
        OpResult::Many(rows) => rows,
        other => panic!("expected rows, got {other:?}"),
    }
}

fn one(result: OpResult) -> i64 {
    match result {
        OpResult::One(OldValue::I64(value)) => value,
        other => panic!("expected an i64 scalar, got {other:?}"),
    }
}

fn none(result: OpResult) {
    match result {
        OpResult::None => {}
        other => panic!("expected no result, got {other:?}"),
    }
}

fn typed<T: Table>(rows: Vec<T>) -> Vec<Vec<NewValue>> {
    rows.into_iter().map(|row| row.to_values()).collect()
}

fn projected<T: Projection>(rows: Vec<T>) -> Vec<Vec<NewValue>> {
    rows.into_iter().map(|row| row.to_values()).collect()
}

fn assert_rows(old: OpResult, new: Vec<Vec<NewValue>>, what: &str) {
    assert_eq!(norm_old(many(old)), norm_new(new), "{what}");
}

const CATALOG: &str = include_str!("../../catalog.lightshade");
const EVENTS: &str = include_str!("../../events.lightshade");
const KV: &str = include_str!("../../kv.lightshade");
const SESSIONS: &str = include_str!("../../sessions.lightshade");
const ORDER_BOOK: &str = include_str!("../../order_book.lightshade");
const INBOX_DIGEST: &str = include_str!("../../inbox_digest.lightshade");

fn product(sku: &str, category: i64, price: i64, name: &str) -> Product {
    Product {
        sku: sku.into(),
        category,
        price,
        name: name.into(),
    }
}

#[test]
fn catalog_matches_the_legacy_engine() {
    let mut old = legacy(CATALOG);
    let mut new = catalog::Db::open_in_memory().unwrap();
    let seed = [
        ("a-1", 7, 100, "alpha"),
        ("a-2", 7, 300, "beta"),
        ("b-1", 9, 150, "gamma"),
        ("b-2", 9, 50, "delta"),
    ];
    for (sku, category, price, name) in seed {
        none(
            old.execute(
                "add_product",
                &[
                    OldValue::Text(sku.into()),
                    OldValue::I64(category),
                    OldValue::I64(price),
                    OldValue::Text(name.into()),
                ],
            )
            .unwrap(),
        );
        new.insert(product(sku, category, price, name)).unwrap();
    }

    assert_rows(
        old.execute("get_product", &[OldValue::Text("a-1".into())])
            .unwrap(),
        typed(new.get_product("a-1".into()).unwrap()),
        "get_product",
    );
    assert_rows(
        old.execute("category_page", &[OldValue::I64(7), OldValue::I64(10)])
            .unwrap(),
        typed(new.category_page(7, 10).unwrap()),
        "category_page",
    );
    assert_rows(
        old.execute("cheap_products", &[OldValue::I64(150)])
            .unwrap(),
        typed(new.cheap_products(150).unwrap()),
        "cheap_products",
    );
    assert_eq!(
        one(old.execute("category_count", &[OldValue::I64(7)]).unwrap()),
        new.category_count(7).unwrap(),
    );
    assert_eq!(
        one(old.execute("product_count", &[]).unwrap()),
        new.product_count().unwrap(),
    );

    none(
        old.execute(
            "update_price",
            &[OldValue::Text("a-1".into()), OldValue::I64(999)],
        )
        .unwrap(),
    );
    assert_eq!(new.update_price("a-1".into(), 999).unwrap(), 1);
    assert_rows(
        old.execute("get_product", &[OldValue::Text("a-1".into())])
            .unwrap(),
        typed(new.get_product("a-1".into()).unwrap()),
        "get_product after update_price",
    );

    none(
        old.execute("remove_product", &[OldValue::Text("a-2".into())])
            .unwrap(),
    );
    assert_eq!(new.remove_product("a-2".into()).unwrap(), 1);
    assert_eq!(
        one(old.execute("product_count", &[]).unwrap()),
        new.product_count().unwrap(),
    );
    assert_rows(
        old.execute("category_page", &[OldValue::I64(7), OldValue::I64(10)])
            .unwrap(),
        typed(new.category_page(7, 10).unwrap()),
        "category_page after remove_product",
    );
}

#[test]
fn events_matches_the_legacy_engine() {
    let mut old = legacy(EVENTS);
    let mut new = events::Db::open_in_memory().unwrap();
    let seed = [
        (1, 1, 100, 1, 10),
        (2, 1, 200, 2, 20),
        (3, 1, 300, 1, 30),
        (4, 2, 150, 1, 40),
        (5, 2, 250, 2, 50),
    ];
    for (id, tenant, ts, kind, value) in seed {
        none(
            old.execute(
                "insert_event",
                &[
                    OldValue::I64(id),
                    OldValue::I64(tenant),
                    OldValue::I64(ts),
                    OldValue::I64(kind),
                    OldValue::I64(value),
                ],
            )
            .unwrap(),
        );
        new.insert(Event {
            id,
            tenant,
            ts,
            kind,
            value,
        })
        .unwrap();
    }

    assert_rows(
        old.execute("tenant_window", &[OldValue::I64(1), OldValue::I64(150)])
            .unwrap(),
        typed(new.tenant_window(1, 150).unwrap()),
        "tenant_window",
    );
    assert_rows(
        old.execute("kind_recent", &[OldValue::I64(1)]).unwrap(),
        typed(new.kind_recent(1).unwrap()),
        "kind_recent",
    );
    assert_eq!(
        one(old.execute("tenant_count", &[OldValue::I64(2)]).unwrap()),
        new.tenant_count(2).unwrap(),
    );

    none(old.execute("purge_event", &[OldValue::I64(3)]).unwrap());
    assert_eq!(new.purge_event(3).unwrap(), 1);
    assert_rows(
        old.execute("tenant_window", &[OldValue::I64(1), OldValue::I64(0)])
            .unwrap(),
        typed(new.tenant_window(1, 0).unwrap()),
        "tenant_window after purge_event",
    );
    assert_eq!(
        one(old.execute("tenant_count", &[OldValue::I64(1)]).unwrap()),
        new.tenant_count(1).unwrap(),
    );
}

#[test]
fn kv_matches_the_legacy_engine() {
    let mut old = legacy(KV);
    let mut new = kv::Db::open_in_memory().unwrap();
    for (id, value) in [(1, "one"), (2, "two"), (3, "three"), (4, "four")] {
        none(
            old.execute("put_kv", &[OldValue::I64(id), OldValue::Text(value.into())])
                .unwrap(),
        );
        new.insert(Kv {
            id,
            value: value.into(),
        })
        .unwrap();
    }

    assert_rows(
        old.execute("get_kv", &[OldValue::I64(2)]).unwrap(),
        typed(new.get_kv(2).unwrap()),
        "get_kv",
    );
    assert_rows(
        old.execute("scan_kv", &[OldValue::I64(2), OldValue::I64(2)])
            .unwrap(),
        typed(new.scan_kv(2, 2).unwrap()),
        "scan_kv",
    );
    assert_eq!(
        one(old.execute("kv_count", &[]).unwrap()),
        new.kv_count().unwrap(),
    );

    none(
        old.execute(
            "update_kv",
            &[OldValue::I64(2), OldValue::Text("TWO".into())],
        )
        .unwrap(),
    );
    assert_eq!(new.update_kv(2, "TWO".into()).unwrap(), 1);
    assert_rows(
        old.execute("get_kv", &[OldValue::I64(2)]).unwrap(),
        typed(new.get_kv(2).unwrap()),
        "get_kv after update_kv",
    );
}

#[test]
fn sessions_matches_the_legacy_engine() {
    let mut old = legacy(SESSIONS);
    let mut new = sessions::Db::open_in_memory().unwrap();
    let seed = [
        (1, 7, 100, true, "t1"),
        (2, 7, 200, false, "t2"),
        (3, 9, 300, true, "t3"),
        (4, 7, 400, true, "t4"),
    ];
    for (id, user, last_seen, active, token) in seed {
        none(
            old.execute(
                "create_session",
                &[
                    OldValue::I64(id),
                    OldValue::I64(user),
                    OldValue::I64(last_seen),
                    OldValue::Bool(active),
                    OldValue::Text(token.into()),
                ],
            )
            .unwrap(),
        );
        new.insert(Session {
            id,
            user,
            last_seen,
            active,
            token: token.into(),
        })
        .unwrap();
    }

    assert_rows(
        old.execute("get_session", &[OldValue::I64(1)]).unwrap(),
        typed(new.get_session(1).unwrap()),
        "get_session",
    );
    assert_eq!(
        one(old.execute("active_sessions", &[OldValue::I64(7)]).unwrap()),
        new.active_sessions(7).unwrap(),
    );

    none(
        old.execute("touch_session", &[OldValue::I64(1), OldValue::I64(999)])
            .unwrap(),
    );
    assert_eq!(new.touch_session(1, 999).unwrap(), 1);
    assert_rows(
        old.execute("get_session", &[OldValue::I64(1)]).unwrap(),
        typed(new.get_session(1).unwrap()),
        "get_session after touch_session",
    );

    none(old.execute("end_session", &[OldValue::I64(2)]).unwrap());
    assert_eq!(new.end_session(2).unwrap(), 1);
    assert_eq!(
        one(old.execute("active_sessions", &[OldValue::I64(7)]).unwrap()),
        new.active_sessions(7).unwrap(),
    );
}

#[test]
fn order_book_matches_the_legacy_engine() {
    let mut old = legacy(ORDER_BOOK);
    let mut new = order_book::Db::open_in_memory().unwrap();
    let seed = [
        (1, 7, 100, 1, 10),
        (2, 7, 200, 2, 20),
        (3, 9, 150, 1, 30),
        (4, 7, 300, 1, 40),
        (5, 9, 250, 2, 50),
    ];
    for (id, customer, created_at, status, total) in seed {
        none(
            old.execute(
                "insert_order",
                &[
                    OldValue::I64(id),
                    OldValue::I64(customer),
                    OldValue::I64(created_at),
                    OldValue::I64(status),
                    OldValue::I64(total),
                ],
            )
            .unwrap(),
        );
        new.insert(Order {
            id,
            customer,
            created_at,
            status,
            total,
        })
        .unwrap();
    }

    assert_rows(
        old.execute("get_order", &[OldValue::I64(2)]).unwrap(),
        typed(new.get_order(2).unwrap()),
        "get_order",
    );
    assert_rows(
        old.execute("customer_orders", &[OldValue::I64(7), OldValue::I64(10)])
            .unwrap(),
        typed(new.customer_orders(7, 10).unwrap()),
        "customer_orders",
    );
    assert_rows(
        old.execute("status_window", &[OldValue::I64(1), OldValue::I64(120)])
            .unwrap(),
        typed(new.status_window(1, 120).unwrap()),
        "status_window",
    );
    assert_rows(
        old.execute("recent_orders", &[OldValue::I64(0)]).unwrap(),
        typed(new.recent_orders(0).unwrap()),
        "recent_orders",
    );
    assert_rows(
        old.execute("customer_total", &[OldValue::I64(7)]).unwrap(),
        projected(new.customer_total(7).unwrap()),
        "customer_total",
    );
    assert_eq!(
        one(old.execute("status_count", &[OldValue::I64(2)]).unwrap()),
        new.status_count(2).unwrap(),
    );
    assert_eq!(
        one(old.execute("order_count", &[]).unwrap()),
        new.order_count().unwrap(),
    );

    none(
        old.execute("mark_status", &[OldValue::I64(1), OldValue::I64(9)])
            .unwrap(),
    );
    assert_eq!(new.mark_status(1, 9).unwrap(), 1);
    assert_rows(
        old.execute("status_window", &[OldValue::I64(9), OldValue::I64(0)])
            .unwrap(),
        typed(new.status_window(9, 0).unwrap()),
        "status_window after mark_status",
    );

    none(old.execute("delete_order", &[OldValue::I64(5)]).unwrap());
    assert_eq!(new.delete_order(5).unwrap(), 1);
    assert_eq!(
        one(old.execute("order_count", &[]).unwrap()),
        new.order_count().unwrap(),
    );
    assert_rows(
        old.execute("customer_total", &[OldValue::I64(9)]).unwrap(),
        projected(new.customer_total(9).unwrap()),
        "customer_total after delete_order",
    );
}

#[test]
fn inbox_digest_matches_the_legacy_engine() {
    let mut old = legacy(INBOX_DIGEST);
    let mut new = inbox_digest::Db::open_in_memory().unwrap();

    assert_rows(
        old.execute("overall", &[]).unwrap(),
        projected(new.overall().unwrap()),
        "overall on an empty table",
    );

    for (id, owner) in [(1, "ada"), (2, "grace")] {
        none(
            old.execute(
                "add_inbox",
                &[OldValue::I64(id), OldValue::Text(owner.into())],
            )
            .unwrap(),
        );
        new.insert(Inbox {
            id,
            owner: owner.into(),
        })
        .unwrap();
    }
    let messages = [
        (1, 1, 100, false, "hello"),
        (2, 1, 200, false, "again"),
        (3, 2, 150, true, "read"),
        (4, 2, 300, false, "latest"),
    ];
    for (id, inbox_id, sent_at, is_read, body) in messages {
        none(
            old.execute(
                "add_message",
                &[
                    OldValue::I64(id),
                    OldValue::I64(inbox_id),
                    OldValue::I64(sent_at),
                    OldValue::Bool(is_read),
                    OldValue::Text(body.into()),
                ],
            )
            .unwrap(),
        );
        new.insert(Msg {
            id,
            inbox_id,
            sent_at,
            is_read,
            body: body.into(),
        })
        .unwrap();
    }

    assert_rows(
        old.execute("get_message", &[OldValue::I64(2)]).unwrap(),
        typed(new.get_message(2).unwrap()),
        "get_message",
    );
    assert_eq!(
        one(old.execute("message_count", &[]).unwrap()),
        new.message_count().unwrap(),
    );
    assert_rows(
        old.execute("unread_digest", &[]).unwrap(),
        projected(new.unread_digest().unwrap()),
        "unread_digest",
    );
    assert_rows(
        old.execute("overall", &[]).unwrap(),
        projected(new.overall().unwrap()),
        "overall",
    );
    assert_rows(
        old.execute("excerpts", &[OldValue::I64(120)]).unwrap(),
        projected(new.excerpts(120).unwrap()),
        "excerpts",
    );

    none(old.execute("mark_read", &[OldValue::I64(2)]).unwrap());
    assert_eq!(new.mark_read(2).unwrap(), 1);
    assert_rows(
        old.execute("unread_digest", &[]).unwrap(),
        projected(new.unread_digest().unwrap()),
        "unread_digest after mark_read",
    );

    none(old.execute("delete_message", &[OldValue::I64(4)]).unwrap());
    assert_eq!(new.delete_message(4).unwrap(), 1);
    assert_rows(
        old.execute("excerpts", &[OldValue::I64(0)]).unwrap(),
        projected(new.excerpts(0).unwrap()),
        "excerpts after delete_message",
    );
    assert_eq!(
        one(old.execute("message_count", &[]).unwrap()),
        new.message_count().unwrap(),
    );
}
