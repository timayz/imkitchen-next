use askama::Template;
use axum::{
    Form,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

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
    pub role: Role,
}

/// A user authorized to share recipes.
#[derive(Debug)]
pub struct Chef;

/// A user with a premium subscription.
#[derive(Debug)]
pub struct Premium;

impl<S: Send + Sync> FromRequestParts<S> for User {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| Redirect::to("/login"))?;
        let role = jar
            .get(SESSION_COOKIE)
            .and_then(|c| Role::parse(c.value()))
            .ok_or_else(|| Redirect::to("/login"))?;
        Ok(User { role })
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Chef {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = User::from_request_parts(parts, state).await?;
        if user.role == Role::Chef {
            Ok(Chef)
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
            Ok(Premium)
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
    let cookie = Cookie::build((SESSION_COOKIE, role.as_str()))
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
