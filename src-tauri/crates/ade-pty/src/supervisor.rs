//! kill 回收路径：closer 先断管道，句柄入队由常驻 harvester 逐个阻塞 join。
//!
//! std 没有 join 超时，且 `join` 一旦执行就只能阻塞到底——flag+condvar 等
//! 「限时 join」方案不可行。故 [`Supervisor::reap`] 采用 harvester 模式：
//!
//! 1. `closer()` 在调用方线程内**同步先执行**（对 PTY 而言即关闭 master：管道
//!    断裂 → reader 线程确定性 EOF，这是 join 能完成的前提；顺序契约见
//!    [`crate::session::Session::kill`]）；
//! 2. 句柄移入 pending 队列，`reap` **立即返回**——绝不在调用方线程上 join；
//! 3. `Supervisor::new()` 起的常驻 harvester 线程从队首逐个阻塞 `join`，
//!    「join(2s) 超时」语义退化为「最终回收」：closer 保证 reader 很快 EOF，
//!    harvester 兜底回收，泄漏线程的形态（目标线程永活）只丢句柄不丢线程安全。
//!
//! pending 上限 [`REAPER_CAP`]：超限 `pop_front` 丢弃最旧句柄（drop
//! `JoinHandle` = detach，线程照常运行到结束，不泄漏不挂起）。上限只约束
//! 「尚未被 harvester 取走」的句柄；正被 join 的那个不在队列内。
//!
//! 进程退出不做优雅关闭：harvester 线程与队列随进程终止（可能仍有句柄未
//! join），进程退出本身会回收一切，无需 Drop 接线。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// pending 队列上限；超限 `pop_front` 丢弃最旧（detach 兜底）。
pub const REAPER_CAP: usize = 64;

/// harvester 与 `reap` 共享的回收队列状态。
#[derive(Default)]
struct Shared {
    pending: Mutex<VecDeque<JoinHandle<()>>>,
    pending_cv: Condvar,
    /// harvester 已完成的 join 次数（join Err——目标线程 panic——同样计为回收完成）。
    joined_total: AtomicUsize,
}

pub struct Supervisor {
    /// 回收队列（名字对齐 brief 接口草图；与 harvester 线程共享）。
    reaper: Arc<Shared>,
}

impl Supervisor {
    /// 创建 Supervisor 并启动常驻 harvester 线程（队列空时阻塞等待）。
    pub fn new() -> Self {
        let reaper = Arc::new(Shared::default());
        let shared = Arc::clone(&reaper);
        std::thread::Builder::new()
            .name("pty-reaper".to_string())
            .spawn(move || harvester_loop(&shared))
            .expect("spawn pty reaper thread");
        Self { reaper }
    }

    /// 回收一个线程：先跑 `closer`（断管道，保证 reader 很快 EOF），再把句柄
    /// 移入 harvester 队列并**立即返回**——不等待 join（语义见模块文档）。
    pub fn reap(&self, closer: impl FnOnce(), handle: JoinHandle<()>) {
        closer();
        let mut pending = self.reaper.pending.lock().expect("reaper mutex poisoned");
        while pending.len() >= REAPER_CAP {
            // drop JoinHandle = detach：最旧的线程失去被 join 的机会，但照常
            // 运行到结束；不泄漏不挂起。
            pending.pop_front();
        }
        pending.push_back(handle);
        drop(pending);
        self.reaper.pending_cv.notify_one();
    }

    /// 尚未被 harvester 取走的句柄数（观测/测试用；正被 join 的不在其内）。
    pub fn pending_len(&self) -> usize {
        self.reaper
            .pending
            .lock()
            .expect("reaper mutex poisoned")
            .len()
    }

    /// harvester 已完成的 join 次数（观测/测试用）。
    pub fn joined_total(&self) -> usize {
        self.reaper.joined_total.load(Ordering::Relaxed)
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

/// 常驻 harvester：队列空则阻塞等待，否则取走队首逐个**阻塞** join 到底。
/// 本线程随进程终止（不做优雅关闭，见模块文档）。
fn harvester_loop(shared: &Shared) {
    loop {
        let handle = {
            let mut pending = shared.pending.lock().expect("reaper mutex poisoned");
            loop {
                match pending.pop_front() {
                    Some(handle) => break handle,
                    None => {
                        pending = shared
                            .pending_cv
                            .wait(pending)
                            .expect("reaper condvar poisoned");
                    }
                }
            }
        };
        // 阻塞到底：目标线程退出（或 panic——join Err 同样视为回收完成）才继续。
        let _ = handle.join();
        shared.joined_total.fetch_add(1, Ordering::Relaxed);
    }
}
