// SPDX-License-Identifier: MIT

use super::*;

fn assert_heavy_queue_is_idle() {
    let queue = lock_queue();
    assert_eq!(queue.running, 0);
    assert!(queue.queued.is_empty());
}

#[test]
fn interactive_requests_jump_ahead_of_queued_prefetches() {
    let context = glib::MainContext::new();
    context.block_on(async {
        let first = request_heavy_preview_permit(Prio::Interactive)
            .acquire()
            .await
            .expect("first permit");
        let mut prefetch = request_heavy_preview_permit(Prio::Prefetch);
        let mut interactive = request_heavy_preview_permit(Prio::Interactive);
        assert!(heavy_preview_is_waiting(&mut prefetch));
        assert!(heavy_preview_is_waiting(&mut interactive));

        drop(first);
        drop(interactive.acquire().await.expect("interactive first"));
        assert!(heavy_preview_is_waiting(&mut prefetch));
        drop(prefetch.acquire().await.expect("prefetch last"));
    });

    assert_heavy_queue_is_idle();
}

#[test]
fn pdf_renders_wait_for_the_active_renderer_and_resume_in_order() {
    let context = glib::MainContext::new();
    context.block_on(async {
        let first = request_heavy_preview_permit(Prio::Interactive)
            .acquire()
            .await
            .expect("first PDF render permit");
        let mut second = request_heavy_preview_permit(Prio::Interactive);
        let mut third = request_heavy_preview_permit(Prio::Interactive);

        assert!(heavy_preview_is_waiting(&mut second));
        assert!(heavy_preview_is_waiting(&mut third));

        drop(first);
        let second = second.acquire().await.expect("second PDF render permit");
        assert!(heavy_preview_is_waiting(&mut third));
        drop(second);
        drop(third.acquire().await.expect("third PDF render permit"));
    });

    assert_heavy_queue_is_idle();
}

#[test]
fn dropping_a_queued_pdf_render_removes_it_without_consuming_a_slot() {
    let context = glib::MainContext::new();
    context.block_on(async {
        let first = request_heavy_preview_permit(Prio::Interactive)
            .acquire()
            .await
            .expect("first PDF render permit");
        let cancelled = request_heavy_preview_permit(Prio::Interactive);
        drop(cancelled);
        drop(first);
    });

    assert_heavy_queue_is_idle();
}
