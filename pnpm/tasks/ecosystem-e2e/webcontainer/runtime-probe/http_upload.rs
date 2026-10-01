use super::host;
use serde_json::{Value, json};

pub(super) async fn check() {
    let child = host(json!({
        "operation": "process.spawn", "program": "node", "args": ["-e", r#"
const http = require('node:http');
const headers = (request, response) => response.end(JSON.stringify({authorization: request.headers.authorization ?? null}));
const other = http.createServer(headers);
const server = http.createServer((request, response) => {
  if (request.url === '/redirect' || request.url === '/same') {
    response.writeHead(302, {location: request.url === '/same' ? '/headers' : 'http://127.0.0.1:' + other.address().port});
    response.end();
    return;
  }
  if (request.url === '/same-userinfo' || request.url === '/cross-userinfo') {
    const port = request.url === '/same-userinfo' ? server.address().port : other.address().port;
    response.writeHead(302, {location: 'http://redirect:secret@127.0.0.1:' + port + '/headers'});
    response.end();
    return;
  }
  if (request.url === '/headers') return headers(request, response);
  let length = 0;
  let valid = true;
  request.on('data', chunk => {
    length += chunk.length;
    valid &&= chunk.every(byte => byte === 255);
  });
  request.on('end', () => {
    response.statusCode = valid && String(length) === request.headers['content-length'] ? 200 : 400;
    response.end(String(length));
  });
});
other.listen(0, '127.0.0.1', () => server.listen(0, '127.0.0.1', () => console.log('http://127.0.0.1:' + server.address().port)));
"#],
    }))
    .await;
    let url = read_line(&child["stdout"]).await;
    let length = 2 * 1024 * 1024 + 17;
    let response = pnpm_http::Client::new()
        .post(url.trim())
        .body(vec![255; length])
        .send()
        .await
        .expect("stream upload larger than the transfer limit")
        .error_for_status()
        .expect("upload content matches")
        .text()
        .await
        .expect("upload byte count");
    assert_eq!(response, length.to_string());
    check_redirect_auth(url.trim()).await;
    check_url_credentials(url.trim()).await;
    for handle in [&child["stdout"], &child["stderr"], &child["handle"]] {
        host(json!({ "operation": "resource.close", "handle": handle })).await;
    }
    println!("HTTP binary upload larger than two MiB passed");
}

async fn check_redirect_auth(url: &str) {
    for (route, authorization) in [
        ("same", json!("Bearer secret")),
        ("redirect", Value::Null),
        ("same-userinfo", json!("Bearer secret")),
        ("cross-userinfo", Value::Null),
    ] {
        let response = pnpm_http::Client::new()
            .get(format!("{url}/{route}"))
            .bearer_auth("secret")
            .send()
            .await
            .expect("redirect request");
        assert_eq!(response.url().username(), "");
        assert_eq!(response.url().password(), None);
        let headers: Value = response.json().await.expect("redirect request headers");
        assert_eq!(headers["authorization"], authorization);
    }
    println!("HTTP redirects preserve same-origin credentials and remove cross-origin credentials");
}

async fn read_line(handle: &Value) -> String {
    let mut bytes = Vec::<u8>::new();
    while !bytes.contains(&b'\n') {
        let chunk = host(json!({ "operation": "stream.read", "handle": handle })).await;
        assert_ne!(chunk["done"], true, "server must report its URL before exiting");
        bytes.extend(
            serde_json::from_value::<Vec<u8>>(chunk["bytes"].clone()).expect("stdout bytes"),
        );
    }
    String::from_utf8(bytes).expect("HTTP server URL is UTF-8")
}

async fn check_url_credentials(url: &str) {
    let authenticated = format!("{}/headers", url.replacen("http://", "http://user:p%40ss@", 1));
    let request = pnpm_http::Client::new()
        .get(authenticated)
        .build()
        .expect("URL credentials");
    assert_eq!(request.url().username(), "");
    assert_eq!(request.url().password(), None);
    let headers: Value = pnpm_http::Client::new()
        .execute(request)
        .await
        .expect("Basic authorization request")
        .json()
        .await
        .expect("authorization response");
    assert_eq!(headers["authorization"], "Basic dXNlcjpwQHNz");
    println!(
        "HTTP URL credentials become Basic authorization and are removed before host dispatch"
    );
}
