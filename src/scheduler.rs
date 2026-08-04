use crate::scraper::{clean_fetch_log, clean_old_articles, scrape_all};
use crate::state::AppState;
use chrono::Utc;

/// Corre un scrape completo y actualiza el estado compartido, protegiendo
/// contra ejecuciones solapadas (si ya hay un scrape en curso, no arranca
/// otro).
pub async fn run_scrape_once(state: AppState) {
    {
        let mut status = state.scrape_state.write().await;
        if status.scraping {
            tracing::info!("[SCRAPER] Scrape ya en curso, se omite esta corrida");
            return;
        }
        status.scraping = true;
        status.started_at = Some(Utc::now().to_rfc3339());
    }

    let result = scrape_all(&state).await;

    let mut status = state.scrape_state.write().await;
    status.scraping = false;
    status.last_finished_at = Some(Utc::now().to_rfc3339());
    match result {
        Ok((total_new, errors)) => {
            status.last_total_new = total_new as i64;
            status.last_errors = errors;
        }
        Err(e) => {
            tracing::error!("[SCRAPER] Scrape falló: {e:#}");
            status.last_total_new = 0;
            status.last_errors = vec![e.to_string()];
        }
    }
    drop(status);

    if let Err(e) = clean_old_articles(&state).await {
        tracing::error!("[SCRAPER] Limpieza falló: {e:#}");
    }
    if let Err(e) = clean_fetch_log(&state).await {
        tracing::error!("[SCRAPER] Limpieza de fetch_log falló: {e:#}");
    }
}

/// Lee `SCRAPE_INTERVAL_MINUTES` (default 15) y lanza el scrape inicial
/// más un loop periódico en background. Equivalente simplificado al cron
/// `*/15 * * * *` del original (sin soporte de expresiones cron
/// arbitrarias, que no hacía falta para este alcance).
pub fn spawn_background_scraper(state: AppState) {
    let interval_minutes: u64 = std::env::var("SCRAPE_INTERVAL_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(15);

    let scrape_on_startup = std::env::var("SCRAPE_ON_STARTUP")
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(true);

    if scrape_on_startup {
        tokio::spawn({
            let state = state.clone();
            async move {
                tracing::info!("[SCHEDULER] Corriendo scrape inicial en background...");
                run_scrape_once(state).await;
            }
        });
    } else {
        tracing::info!("[SCHEDULER] Scrape inicial desactivado (SCRAPE_ON_STARTUP=0)");
    }

    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(interval_minutes * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // primer tick es inmediato, lo consumimos
        loop {
            ticker.tick().await;
            tracing::info!("[SCHEDULER] Scrape periódico ({interval_minutes} min)");
            run_scrape_once(state.clone()).await;
        }
    });
}
