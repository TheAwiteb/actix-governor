use crate::extractor::GovernorExtractor;
use crate::{GovernorResult, KeyExtractor, SimpleKeyExtractionError};
use actix_http::header::{HeaderName, HeaderValue};
use actix_web::{
    dev::Service,
    http::{
        header::{self, ContentType},
        StatusCode,
    },
    web, App, HttpResponse, HttpResponseBuilder, Responder,
};

#[test]
fn builder_test() {
    use crate::GovernorConfigBuilder;

    let mut builder = GovernorConfigBuilder::default();
    builder
        .period(crate::DEFAULT_PERIOD)
        .burst_size(crate::DEFAULT_BURST_SIZE);

    assert_eq!(GovernorConfigBuilder::default(), builder);

    let mut builder1 = builder.clone();
    builder1.per_millisecond(5000);
    let builder2 = builder.per_second(5);

    assert_eq!(&builder1, builder2);
}

async fn hello() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

#[actix_rt::test]
async fn test_server() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;

    let config = GovernorConfigBuilder::default()
        .per_millisecond(90)
        .burst_size(2)
        .finish()
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello)),
    )
    .await;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80u16);

    // First request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);

    // Second request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);

    // Third request -> Over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    assert_eq!(test.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-after"))
            .unwrap(),
        "0"
    );

    // Replenish one element by waiting for >90ms
    let sleep_time = std::time::Duration::from_millis(100);
    std::thread::sleep(sleep_time);

    // First request after reset
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);

    // Second request after reset -> Again over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    assert_eq!(test.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-after"))
            .unwrap(),
        "0"
    );
    let body = actix_web::body::to_bytes(test.into_body()).await.unwrap();
    assert_eq!(body, "Too many requests, retry in 0s");
}

#[actix_rt::test]
async fn test_method_filter() {
    use crate::{Governor, GovernorConfigBuilder, Method};
    use actix_web::test;

    let config = GovernorConfigBuilder::default()
        .per_millisecond(90)
        .burst_size(2)
        .methods(vec![Method::GET])
        .finish()
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello))
            .route("/", web::post().to(hello)),
    )
    .await;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80u16);

    // First request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);

    // Second request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);

    // Third request -> Over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    assert_eq!(test.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-after"))
            .unwrap(),
        "0"
    );

    // Fourth request, now a POST request
    // This one is ignored by the ratelimit
    let req = test::TestRequest::post()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
}

#[actix_rt::test]
async fn test_server_use_headers() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;

    let config = GovernorConfigBuilder::default()
        .per_millisecond(90)
        .burst_size(2)
        .use_headers()
        .finish()
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello)),
    )
    .await;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80u16);

    // First request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "1"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-after"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Second request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "0"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-after"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Third request -> Over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    assert_eq!(test.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-after"))
            .unwrap(),
        "0"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "0"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Replenish one element by waiting for >90ms
    let sleep_time = std::time::Duration::from_millis(100);
    std::thread::sleep(sleep_time);

    // First request after reset
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "0"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-after"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Second request after reset -> Again over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    assert_eq!(test.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-after"))
            .unwrap(),
        "0"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "0"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    let body = actix_web::body::to_bytes(test.into_body()).await.unwrap();
    assert_eq!(body, "Too many requests, retry in 0s");
}

#[actix_rt::test]
async fn test_method_filter_use_headers() {
    use crate::{Governor, GovernorConfigBuilder, Method};
    use actix_web::test;

    let config = GovernorConfigBuilder::default()
        .per_millisecond(90)
        .burst_size(2)
        .methods(vec![Method::GET])
        .use_headers()
        .finish()
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello))
            .route("/", web::post().to(hello)),
    )
    .await;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80u16);

    // First request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "1"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-after"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Second request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "0"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-after"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Third request -> Over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    assert_eq!(test.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-after"))
            .unwrap(),
        "0"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-limit"))
            .unwrap(),
        "2"
    );
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-remaining"))
            .unwrap(),
        "0"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-whitelisted"))
        .is_none());

    // Fourth request, now a POST request
    // This one is ignored by the ratelimit
    let req = test::TestRequest::post()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    assert_eq!(test.status(), StatusCode::OK);
    assert_eq!(
        test.headers()
            .get(HeaderName::from_static("x-ratelimit-whitelisted"))
            .unwrap(),
        "true"
    );
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-limit"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-remaining"))
        .is_none());
    assert!(test
        .headers()
        .get(HeaderName::from_static("x-ratelimit-after"))
        .is_none());
}

#[actix_rt::test]
async fn test_json_error_response() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FooKeyExtractor;

    impl KeyExtractor for FooKeyExtractor {
        type Key = String;
        type KeyExtractionError = SimpleKeyExtractionError<String>;

        fn extract(
            &self,
            _req: &actix_web::dev::ServiceRequest,
        ) -> Result<Self::Key, Self::KeyExtractionError> {
            Ok("test".to_owned())
        }

        fn exceed_rate_limit_response(
            &self,
            _negative: &governor::NotUntil<governor::clock::QuantaInstant>,
            mut response: HttpResponseBuilder,
        ) -> HttpResponse {
            response
                .content_type(ContentType::json())
                .body(r#"{"msg":"Test"}"#)
        }
    }

    let config = GovernorConfigBuilder::default()
        .burst_size(2)
        .per_second(3)
        .key_extractor(FooKeyExtractor)
        .finish()
        .unwrap();
    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello)),
    )
    .await;

    // First request
    let req = test::TestRequest::get().uri("/").to_request();
    assert_eq!(test::call_service(&app, req).await.status(), StatusCode::OK);
    // Second request
    let req = test::TestRequest::get().uri("/").to_request();
    assert_eq!(test::call_service(&app, req).await.status(), StatusCode::OK);
    // Third request
    let err_req = test::TestRequest::get().uri("/").to_request();
    let err_res = app.call(err_req).await.unwrap();
    assert_eq!(
        err_res.headers().get(header::CONTENT_TYPE).unwrap(),
        HeaderValue::from_static("application/json")
    );
    let body = actix_web::body::to_bytes(err_res.into_body())
        .await
        .unwrap();
    assert_eq!(body, "{\"msg\":\"Test\"}".to_owned());
}

#[actix_rt::test]
async fn test_forbidden_response_error() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FooKeyExtractor;

    impl KeyExtractor for FooKeyExtractor {
        type Key = String;
        type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

        fn extract(
            &self,
            _req: &actix_web::dev::ServiceRequest,
        ) -> Result<Self::Key, Self::KeyExtractionError> {
            Err(SimpleKeyExtractionError::new("test").set_status_code(StatusCode::FORBIDDEN))
        }
    }

    let config = GovernorConfigBuilder::default()
        .burst_size(2)
        .per_second(3)
        .key_extractor(FooKeyExtractor)
        .finish()
        .unwrap();
    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello)),
    )
    .await;

    // First request
    let req = test::TestRequest::get().uri("/").to_request();
    let err_res = app.call(req).await.unwrap_err();
    assert_eq!(
        err_res.as_response_error().status_code(),
        StatusCode::FORBIDDEN
    );
}

#[actix_rt::test]
async fn test_html_error_response() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FooKeyExtractor;

    impl KeyExtractor for FooKeyExtractor {
        type Key = String;
        type KeyExtractionError = SimpleKeyExtractionError<String>;

        fn extract(
            &self,
            _req: &actix_web::dev::ServiceRequest,
        ) -> Result<Self::Key, Self::KeyExtractionError> {
            Ok("test".to_owned())
        }

        fn exceed_rate_limit_response(
            &self,
            _negative: &governor::NotUntil<governor::clock::QuantaInstant>,
            mut response: HttpResponseBuilder,
        ) -> HttpResponse {
            response.content_type(ContentType::html()).body(
                r#"<!DOCTYPE html><html lang="en"><head></head><body><h1>Rate limit error</h1></body></html>"#
            )
        }
    }

    let config = GovernorConfigBuilder::default()
        .burst_size(2)
        .per_second(3)
        .key_extractor(FooKeyExtractor)
        .finish()
        .unwrap();
    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello)),
    )
    .await;

    // First request
    let req = test::TestRequest::get().uri("/").to_request();
    assert_eq!(test::call_service(&app, req).await.status(), StatusCode::OK);
    // Second request
    let req = test::TestRequest::get().uri("/").to_request();
    assert_eq!(test::call_service(&app, req).await.status(), StatusCode::OK);
    // Third request
    let err_req = test::TestRequest::get().uri("/").to_request();
    let err_res = app.call(err_req).await.unwrap();
    assert_eq!(
        err_res.headers().get(header::CONTENT_TYPE).unwrap(),
        HeaderValue::from_static("text/html; charset=utf-8")
    );
    let body = actix_web::body::to_bytes(err_res.into_body())
        .await
        .unwrap();
    assert_eq!(body,"<!DOCTYPE html><html lang=\"en\"><head></head><body><h1>Rate limit error</h1></body></html>".to_owned());
}

#[actix_rt::test]
async fn test_network_authentication_required_response_error() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FooKeyExtractor;

    impl KeyExtractor for FooKeyExtractor {
        type Key = String;
        type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

        fn extract(
            &self,
            _req: &actix_web::dev::ServiceRequest,
        ) -> Result<Self::Key, Self::KeyExtractionError> {
            Err(SimpleKeyExtractionError::new("test")
                .set_status_code(StatusCode::NETWORK_AUTHENTICATION_REQUIRED))
        }
    }

    let config = GovernorConfigBuilder::default()
        .burst_size(2)
        .per_second(3)
        .key_extractor(FooKeyExtractor)
        .finish()
        .unwrap();
    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(hello)),
    )
    .await;

    // First request
    let req = test::TestRequest::get().uri("/").to_request();
    let err_res = app.call(req).await.unwrap_err();
    assert_eq!(
        err_res.as_response_error().status_code(),
        StatusCode::NETWORK_AUTHENTICATION_REQUIRED
    );
}

async fn permissive_route(GovernorExtractor(result): GovernorExtractor) -> impl Responder {
    match result {
        GovernorResult::Ok {
            burst_size,
            remaining,
        } => format!("Ok: {:?} {:?}", burst_size, remaining),
        GovernorResult::Wait { wait, burst_size } => format!("Wait: {} {:?}", wait, burst_size),
        GovernorResult::Whitelisted => "Whitelisted".into(),
        GovernorResult::Err(e) => format!("Err: {}", e),
    }
}

#[actix_rt::test]
async fn test_server_permissive() {
    use crate::{Governor, GovernorConfigBuilder};
    use actix_web::test;
    use actix_web::web::Bytes;

    let config = GovernorConfigBuilder::default()
        .per_millisecond(90)
        .burst_size(2)
        .permissive(true)
        .finish()
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Governor::new(&config))
            .route("/", web::get().to(permissive_route)),
    )
    .await;

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80u16);

    // First request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    let body = actix_web::body::to_bytes(test.into_body()).await.unwrap();
    assert_eq!(body, Bytes::from_static(b"Ok: None None"));

    // Second request
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    let body = actix_web::body::to_bytes(test.into_body()).await.unwrap();
    assert_eq!(body, Bytes::from_static(b"Ok: None None"));

    // Third request -> Over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    let body = actix_web::body::to_bytes(test.into_body()).await.unwrap();
    assert_eq!(body, Bytes::from_static(b"Wait: 0 None"));

    // Replenish one element by waiting for >90ms
    let sleep_time = std::time::Duration::from_millis(100);
    std::thread::sleep(sleep_time);

    // First request after reset
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = test::call_service(&app, req).await;
    let text = test::read_body(test).await;
    assert_eq!(text, Bytes::from_static(b"Ok: None None"));

    // Second request after reset -> Again over limit, returns Error
    let req = test::TestRequest::get()
        .peer_addr(addr)
        .uri("/")
        .to_request();
    let test = app.call(req).await.unwrap();
    let body = actix_web::body::to_bytes(test.into_body()).await.unwrap();
    assert_eq!(body, Bytes::from_static(b"Wait: 0 None"));
}
