use std::future::Future;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn run_ordered_with_limit<T, R, F, Fut>(
    items: Vec<T>,
    limit: usize,
    worker: F,
) -> Result<Vec<R>, String>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let concurrency_limit = limit.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency_limit));
    let worker = Arc::new(worker);
    let mut tasks = JoinSet::new();
    let total = items.len();

    for (index, item) in items.into_iter().enumerate() {
        let semaphore = semaphore.clone();
        let worker = worker.clone();
        tasks.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .map_err(|e| format!("Failed to acquire concurrency permit: {}", e))?;
            let result = worker(item).await;
            Ok::<(usize, R), String>((index, result))
        });
    }

    let mut ordered_results: Vec<Option<R>> = (0..total).map(|_| None).collect();
    while let Some(joined) = tasks.join_next().await {
        let task_result = joined.map_err(|e| format!("Concurrent task failed: {}", e))?;
        let (index, result) = task_result?;
        ordered_results[index] = Some(result);
    }

    Ok(ordered_results
        .into_iter()
        .map(|result| result.expect("all ordered task slots should be populated"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::run_ordered_with_limit;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn run_ordered_with_limit_preserves_input_order_and_limit() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let items = vec![
            ("first", 40_u64),
            ("second", 10_u64),
            ("third", 30_u64),
            ("fourth", 5_u64),
        ];

        let results = run_ordered_with_limit(items, 2, {
            let active = active.clone();
            let max_active = max_active.clone();
            move |(label, delay_ms)| {
                let active = active.clone();
                let max_active = max_active.clone();
                async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    label
                }
            }
        })
        .await
        .expect("ordered concurrency helper should succeed");

        assert_eq!(results, vec!["first", "second", "third", "fourth"]);
        assert_eq!(max_active.load(Ordering::SeqCst), 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
