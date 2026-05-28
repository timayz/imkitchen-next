//! Cookie-based session auth.
//!
//! The session cookie is `imkitchen_session=<role>:<ulid>`. Role gates which
//! pages a user can reach (`Chef`-only, `Premium`-only); the ULID is the
//! per-login owner id used to scope user-owned aggregates (recipes, import
//! jobs, …).
//!
//! Identity is *per login*: the ULID is regenerated on every `POST /login`
//! and lost on `POST /logout`. There is no users table yet; replacing this
//! stub with a real user store later is what unlocks "your recipes survive
//! logout". The cookie format stays compatible: future versions can keep
//! `<role>:<id>` and just point `id` at a real user record.

use askama::Template;
use axum::{
    Form,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use ulid::Ulid;

pub const SESSION_COOKIE: &str = "imkitchen_session";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Chef,
    Premium,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Chef => "chef",
            Role::Premium => "premium",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Role::User),
            "chef" => Some(Role::Chef),
            "premium" => Some(Role::Premium),
            _ => None,
        }
    }
}

/// Any signed-in user. A Chef or Premium session is also a User.
#[derive(Debug)]
pub struct User {
    /// Per-login ULID. Stable across requests *within one session*; a fresh
    /// login produces a new id. Use this to scope user-owned aggregates.
    pub id: String,
    pub role: Role,
}

/// A user authorized to share recipes.
#[derive(Debug)]
pub struct Chef {
    pub id: String,
}

/// A user with a premium subscription.
#[derive(Debug)]
pub struct Premium {
    pub id: String,
}

/// Split `<role>:<id>` cookie value. Returns `None` for any other shape so
/// the extractor falls through to the missing-cookie path (redirect to
/// `/login`).
fn parse_session(value: &str) -> Option<(Role, String)> {
    let (role_str, id) = value.split_once(':')?;
    let role = Role::parse(role_str)?;
    if id.is_empty() {
        return None;
    }
    Some((role, id.to_owned()))
}

impl<S: Send + Sync> FromRequestParts<S> for User {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| Redirect::to("/login"))?;
        let (role, id) = jar
            .get(SESSION_COOKIE)
            .and_then(|c| parse_session(c.value()))
            .ok_or_else(|| Redirect::to("/login"))?;
        Ok(User { id, role })
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Chef {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = User::from_request_parts(parts, state).await?;
        if user.role == Role::Chef {
            Ok(Chef { id: user.id })
        } else {
            Err(Redirect::to("/login"))
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Premium {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = User::from_request_parts(parts, state).await?;
        if user.role == Role::Premium {
            Ok(Premium { id: user.id })
        } else {
            Err(Redirect::to("/login"))
        }
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage;

pub async fn login_page() -> Response {
    render(LoginPage)
}

#[derive(Deserialize)]
pub struct LoginForm {
    role: String,
}

pub async fn login_submit(jar: CookieJar, Form(form): Form<LoginForm>) -> Response {
    let Some(role) = Role::parse(&form.role) else {
        return (StatusCode::BAD_REQUEST, "unknown role").into_response();
    };
    let id = Ulid::new().to_string();
    let value = format!("{}:{}", role.as_str(), id);
    let cookie = Cookie::build((SESSION_COOKIE, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .build();
    (jar.add(cookie), Redirect::to("/")).into_response()
}

pub async fn logout(jar: CookieJar) -> Response {
    let cookie = Cookie::build(SESSION_COOKIE).path("/").build();
    (jar.remove(cookie), Redirect::to("/login")).into_response()
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_session_round_trip() {
        let (role, id) = parse_session("chef:01HZ8K7P00000000000000000A").unwrap();
        assert_eq!(role, Role::Chef);
        assert_eq!(id, "01HZ8K7P00000000000000000A");
    }

    #[test]
    fn parse_session_rejects_missing_id() {
        assert!(parse_session("chef:").is_none());
    }

    #[test]
    fn parse_session_rejects_missing_colon() {
        assert!(parse_session("chef").is_none());
    }

    #[test]
    fn parse_session_rejects_unknown_role() {
        assert!(parse_session("admin:01HZ").is_none());
    }

    #[test]
    fn parse_session_keeps_id_intact_with_extra_colons() {
        // First colon is the separator; anything after is the id verbatim.
        let (role, id) = parse_session("user:abc:def").unwrap();
        assert_eq!(role, Role::User);
        assert_eq!(id, "abc:def");
    }
}
