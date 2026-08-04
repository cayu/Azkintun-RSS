pub mod articles;
pub mod auth;
pub mod export;
pub mod folders;
pub mod health;
pub mod import;
pub mod scrape;
pub mod sources;

use crate::state::AppState;
use axum::Router;

/// Ensambla el router completo:
///  - rutas públicas (health, login) accesibles sin token
///  - rutas protegidas (todo lo demás) detrás del middleware `require_auth`
pub fn all_routes(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .merge(health::public_router())
        .merge(auth::public_router());

    let protected = Router::new()
        .merge(health::protected_router())
        .merge(auth::protected_router())
        .merge(folders::router())
        .merge(sources::router())
        .merge(articles::router())
        .merge(import::router())
        .merge(export::router())
        .merge(scrape::router())
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            crate::auth::require_auth,
        ));

    public.merge(protected)
}
