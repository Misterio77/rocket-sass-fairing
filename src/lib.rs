//! Easily compile and serve a sass/scss style sheet through Rocket.
//! ```rust
//! use rocket::{launch, get, routes};
//! use rocket_sass_fairing::SassSheet;
//!
//! #[rocket::main]
//! async fn main() {
//!     rocket::build()
//!         .attach(SassSheet::fairing())
//!         .mount("/", routes![style])
//!         .launch()
//!         .await;
//! }
//!
//! #[get("/assets/style.css")]
//! async fn style(sheet: &SassSheet) -> &SassSheet { sheet }
//! 
//! # use rocket::local::blocking::Client;
//! # use rocket::http::Status;
//! 
//! # #[test]
//! # fn rewrites() {
//! #     let client = Client::tracked(rocket()).expect("valid rocket instance");
//! #     let response = client.get("/assets/style.css").dispatch();
//! #     assert_eq!(response.status(), Status::Ok);
//! #     assert_eq!(response.into_string().unwrap(), "a b{color:a b}");
//! # }
//! ```
use normpath::PathExt;
use rocket::{
    error,
    fairing::{self, Fairing, Info, Kind},
    http::ContentType,
    info, info_,
    outcome::IntoOutcome,
    request::{self, FromRequest, Request},
    response::{self, Responder, Response},
    Build, Orbit, Rocket,
};
use std::path::PathBuf;

pub struct SassSheet {
    content: String,
    cache_max_age: i32,
    path: PathBuf,
}

impl SassSheet {
    pub fn fairing() -> impl Fairing {
        SassSheetFairing
    }
}

struct SassSheetFairing;

#[rocket::async_trait]
impl Fairing for SassSheetFairing {
    fn info(&self) -> Info {
        Info {
            kind: Kind::Ignite | Kind::Liftoff,
            name: "Sass Sheet",
        }
    }

    async fn on_ignite(&self, rocket: Rocket<Build>) -> fairing::Result {
        use rocket::figment::value::magic::RelativePathBuf;

        let configured_path = rocket
            .figment()
            .extract_inner::<RelativePathBuf>("sass_sheet_path")
            .map(|path| path.relative());

        let relative_path = match configured_path {
            Ok(path) => path,
            Err(e) if e.missing() => "assets/style.scss".into(),
            Err(e) => {
                rocket::config::pretty_print_error(e);
                return Err(rocket);
            }
        };

        let path = match relative_path.normalize() {
            Ok(path) => path.into_path_buf(),
            Err(e) => {
                error!(
                    "Invalid sass sheet file '{}': {}.",
                    relative_path.display(),
                    e
                );
                return Err(rocket);
            }
        };

        info!("Compiling sass... file {}", path.display());
        println!("Compiling sass... file {}", path.display());

        let options = grass::Options::default().style(grass::OutputStyle::Compressed);
        let compiled_css = match grass::from_path(&path.to_string_lossy(), &options) {
            Ok(css) => css,
            Err(e) => {
                error!("Couldn't compile sass: {}", e);
                return Err(rocket);
            }
        };

        let cache_max_age = rocket
            .figment()
            .extract_inner::<i32>("assets_max_age")
            .unwrap_or(86400);

        Ok(rocket.manage(SassSheet {
            content: compiled_css,
            cache_max_age,
            path,
        }))
    }

    async fn on_liftoff(&self, rocket: &Rocket<Orbit>) {
        use rocket::{figment::Source, log::PaintExt, yansi::Paint};

        let state = rocket
            .state::<SassSheet>()
            .expect("SassSheet registered in on_ignite");

        info!("{}{}:", Paint::emoji("📐 "), Paint::magenta("Assets"));
        info_!("sheet path: {}", Paint::white(Source::from(&*state.path)));
        info_!("cache max age: {}", Paint::white(state.cache_max_age));
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for &'r SassSheet {
    type Error = ();
    async fn from_request(req: &'r Request<'_>) -> request::Outcome<Self, ()> {
        req.rocket().state::<SassSheet>().or_forward(())
    }
}
impl<'r, 'o: 'r> Responder<'r, 'o> for &'o SassSheet {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'o> {
        let content: &str = self.content.as_ref();
        Response::build_from(content.respond_to(req)?)
            .header(ContentType::CSS)
            .raw_header("Cache-control", format!("max-age={}", self.cache_max_age))
            .ok()
    }
}
