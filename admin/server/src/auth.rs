use askama::Template;
use axum::{
    Form,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

pub const SESSION_COOKIE: &str = "imkitchen_admin_session";
const ADMIN_VALUE: &str = "admin";

#[derive(Debug)]
pub struct Admin;

impl<S: Send + Sync> FromRequestParts<S> for Admin {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| Redirect::to("/login"))?;
        let ok = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value() == ADMIN_VALUE)
            .unwrap_or(false);
        if ok {
            Ok(Admin)
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
    if form.role != ADMIN_VALUE {
        return (StatusCode::BAD_REQUEST, "unknown role").into_response();
    }
    let cookie = Cookie::build((SESSION_COOKIE, ADMIN_VALUE))
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
