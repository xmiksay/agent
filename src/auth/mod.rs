pub mod handlers;
pub mod middleware;
pub mod operations;
pub mod store;
pub mod waiter;

#[allow(unused_imports)]
pub use store::{AuthRequest, AuthStatus};
#[allow(unused_imports)]
pub use waiter::AuthWaiter;

use subtle::ConstantTimeEq;

/// Constant-time check of a presented token against the configured one. When no
/// token is configured (typical for dev), everything is allowed. Shared by the
/// `/api/*` bearer middleware (header) and the WebSocket handler (query param).
pub fn token_ok(expected: Option<&str>, presented: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    match presented {
        Some(p) => expected.as_bytes().ct_eq(p.as_bytes()).unwrap_u8() == 1,
        None => false,
    }
}
