//! M3 acceptance — calendar timebox slots from an editable weekly template.
//!
//! Proves the seeded weekly cadence, slot generation per weekday, goal
//! assignment, linking a slot to a work item (generalized relationship), and
//! that editing the template changes future generation.

use std::collections::HashMap;

use korg_core::repo::{create_work_item, neighbors, relate, NewWorkItem};
use korg_core::slots::{
    generate_slots, list_slots, list_templates, set_slot_goal, set_weekly_template, NewTemplateSlot,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;
use time::macros::date;
use time::Date;

async fn fresh_korg() -> (impl Sized, PgPool) {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect");
    korg_core::migrator().run(&pool).await.expect("migrate");
    (container, pool)
}

fn durations_on(slots: &[korg_core::slots::Slot], day: Date) -> Vec<i32> {
    let mut d: Vec<i32> = slots
        .iter()
        .filter(|s| s.slot_date == day)
        .map(|s| s.duration_minutes)
        .collect();
    d.sort();
    d
}

#[tokio::test]
async fn slots_weekly_template_generate_assign_and_edit() {
    let (_c, pool) = fresh_korg().await;

    // Seeded default template: 5*2 + 3 + 3 = 16 rows.
    let templates = list_templates(&pool).await.expect("templates");
    assert_eq!(templates.len(), 16, "seeded template row count");
    let mut per_dow: HashMap<i16, Vec<i32>> = HashMap::new();
    for t in &templates {
        per_dow.entry(t.dow).or_default().push(t.duration_minutes);
    }
    for d in &mut per_dow.values_mut() {
        d.sort();
    }
    assert_eq!(per_dow[&1], vec![30, 60], "Monday template"); // weekday
    assert_eq!(per_dow[&6], vec![30, 120, 120], "Saturday template");
    assert_eq!(per_dow[&0], vec![30, 30, 60], "Sunday template");

    // Generate a full Mon..Sun week (2024-01-01 is a Monday).
    let monday = date!(2024 - 01 - 01);
    let created = generate_slots(&pool, monday, 7).await.expect("generate");
    assert_eq!(created, 16, "one week generates 16 slots");

    let week = list_slots(&pool, monday, date!(2024 - 01 - 07))
        .await
        .expect("list slots");
    // Mon-Fri -> [30,60]
    for offset in 0..5 {
        let day = monday.checked_add(time::Duration::days(offset)).unwrap();
        assert_eq!(durations_on(&week, day), vec![30, 60], "weekday {day} durations");
    }
    assert_eq!(
        durations_on(&week, date!(2024 - 01 - 06)),
        vec![30, 120, 120],
        "Saturday durations"
    );
    assert_eq!(
        durations_on(&week, date!(2024 - 01 - 07)),
        vec![30, 30, 60],
        "Sunday durations"
    );

    // Put a small goal into a slot, and link that slot to a work item.
    let wi = create_work_item(
        &pool,
        NewWorkItem {
            project_id: None,
            area_id: None,
            wi_type: "task".into(),
            wi_status: "open".into(),
            wi_tshirt: "S".into(),
            sprint: None,
            title: "Learn rmcp".into(),
            content: "spike".into(),
            details: None,
            category: None,
            tags: vec![],
        },
    )
    .await
    .expect("work item");

    let first_slot = week[0].node_id;
    set_slot_goal(&pool, first_slot, Some("Read rmcp docs"))
        .await
        .expect("set goal");
    relate(&pool, first_slot, wi.node_id, "advances")
        .await
        .expect("relate slot-wi");

    let reread = list_slots(&pool, monday, monday).await.expect("reread");
    assert_eq!(reread[0].goal.as_deref(), Some("Read rmcp docs"), "goal persisted");
    let ns = neighbors(&pool, first_slot).await.expect("neighbors");
    assert_eq!(ns.len(), 1);
    assert_eq!(ns[0].node_id, wi.node_id);
    assert_eq!(ns[0].kind, "workitem");
    assert_eq!(ns[0].label, "advances");

    // Edit the template (free time changed): Monday becomes a single 45-min slot.
    let mut edited: Vec<NewTemplateSlot> = templates
        .iter()
        .filter(|t| t.dow != 1)
        .map(|t| NewTemplateSlot {
            dow: t.dow,
            position: t.position,
            duration_minutes: t.duration_minutes,
            label: t.label.clone(),
        })
        .collect();
    edited.push(NewTemplateSlot {
        dow: 1,
        position: 0,
        duration_minutes: 45,
        label: None,
    });
    set_weekly_template(&pool, &edited).await.expect("edit template");

    // Regenerate a later Monday and confirm the change took effect.
    let next_monday = date!(2024 - 01 - 08);
    generate_slots(&pool, next_monday, 1).await.expect("regen");
    let next = list_slots(&pool, next_monday, next_monday).await.expect("list next");
    assert_eq!(durations_on(&next, next_monday), vec![45], "edited Monday template");
}
