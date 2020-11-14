use futures::FutureExt;
use rspotify::{
    client::Spotify,
    model::search::SearchResult,
    oauth2::{SpotifyClientCredentials, SpotifyOAuth},
    senum::SearchType,
    util::get_token_by_code,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use warp::{
    http::{header::SET_COOKIE, Uri},
    reply::Reply,
    Filter,
};

#[derive(Serialize, Deserialize, Debug)]
struct CallbackData {
    state: String,
    code: String,
}

#[derive(Serialize, Deserialize)]
struct Search {
    q: String,
}

#[derive(Serialize, Deserialize)]
struct Add {
    id: String,
}

#[tokio::main]
async fn main() {
    let oauth = Arc::new(Mutex::new(
        SpotifyOAuth::default()
            .scope("user-modify-playback-state")
            .client_id("CLIENT_ID")
            .client_secret("CLIENT_SECRET")
            .redirect_uri("http://YOUR_DOMAIN/callback")
            .build(),
    ));
    let client_instances = Arc::new(Mutex::new(HashMap::new()));
    let client_codes = Arc::new(Mutex::new(HashMap::new()));
    let client_instances_a = client_instances.clone();
    let client_instances_b = client_instances.clone();
    let client_instances_c = client_instances.clone();
    let queues = Arc::new(Mutex::new(HashMap::new()));
    let queues_a = queues.clone();
    let queues_b = queues.clone();
    let client_codes_a = client_codes.clone();
    let client_codes_b = client_codes.clone();
    let client_codes_c = client_codes.clone();
    let oauth_a = oauth.clone();
    let callback = warp::get()
        .and(warp::path("callback"))
        .and(warp::query::<CallbackData>())
        .and_then(move |p: CallbackData| {
            let oauth = oauth.clone();
            let queues = queues.clone();
            let client_instances = client_instances.clone();
            let client_codes = client_codes.clone();
            async move {
                let client_credential = SpotifyClientCredentials::default().token_info(
                    get_token_by_code(&mut *oauth.lock().await, &p.code)
                        .await
                        .ok_or("auth token invalid".to_owned())?,
                );
                let spotify = Spotify::default()
                    .client_credentials_manager(client_credential)
                    .build();
                let user = spotify.current_user().await;
                let user = user.map_err(|_| "no user".to_owned())?.id;
                let uuid = Uuid::new_v4();
                client_instances.lock().await.insert(user.clone(), spotify);
                client_codes.lock().await.insert(uuid, user.clone());
                let mut response = warp::reply::with_header(
                    warp::redirect::temporary(Uri::from_static("/")),
                    "set-cookie",
                    format!("user={}", uuid),
                )
                .into_response();
                let queue_id = Uuid::new_v4();
                queues
                    .lock()
                    .await
                    .insert(format!("{}{}", user, queue_id), user.clone());
                response
                    .headers_mut()
                    .append(SET_COOKIE, format!("id={}", user).parse().unwrap());
                response.headers_mut().append(
                    SET_COOKIE,
                    format!("queue_id={}{}", user, queue_id).parse().unwrap(),
                );
                Ok::<_, String>(response)
            }
            .map(|e| -> Result<_, std::convert::Infallible> {
                match e {
                    Ok(data) => Ok(data.into_response()),
                    Err(data) => Ok(data.into_response()),
                }
            })
        });
    warp::serve(
        warp::get()
            .and(
                warp::cookie("user")
                    .and_then(move |user: String| {
                        let client_codes = client_codes_a.clone();
                        async move {
                            if client_codes.lock().await.contains_key(
                                &Uuid::parse_str(&user).map_err(|_| warp::reject::not_found())?,
                            ) {
                                Ok(())
                            } else {
                                Err(warp::reject::not_found())
                            }
                        }
                    })
                    .untuple_one()
                    .and(
                        warp::path!("queue" / String / "add")
                            .and(warp::query::<Add>())
                            .and_then(move |name, query: Add| {
                                let clients = client_instances_a.clone();
                                let queues = queues_a.clone();
                                async move {
                                    let clients = clients.lock().await;
                                    let queues = queues.lock().await;
                                    if let Some(client) = queues.get(&name) {
                                        if let Some(spotify) = clients.get(client) {
                                            let _ = spotify
                                                .add_item_to_queue(
                                                    format!("spotify:track:{}", query.id),
                                                    None,
                                                )
                                                .await
                                                .unwrap();
                                            Ok("success".into_response())
                                        } else {
                                            Ok::<_, std::convert::Infallible>(
                                                warp::reply::html(include_str!(
                                                    "../static/404.html"
                                                ))
                                                .into_response(),
                                            )
                                        }
                                    } else {
                                        Ok::<_, std::convert::Infallible>(
                                            warp::reply::html(include_str!("../static/404.html"))
                                                .into_response(),
                                        )
                                    }
                                }
                            }),
                    )
                    .or(warp::path!("queue" / String).and_then(move |name: String| {
                        let clients = client_instances_c.clone();
                        let queues = queues_b.clone();
                        async move {
                            let clients = clients.lock().await;
                            let queues = queues.lock().await;
                            if let Some(client) = queues.get(&name) {
                                if let Some(_) = clients.get(client) {
                                    Ok(warp::reply::html(include_str!("../static/queue.html"))
                                        .into_response())
                                } else {
                                    Ok::<_, std::convert::Infallible>(
                                        warp::reply::html(include_str!("../static/404.html"))
                                            .into_response(),
                                    )
                                }
                            } else {
                                Ok::<_, std::convert::Infallible>(
                                    warp::reply::html(include_str!("../static/404.html"))
                                        .into_response(),
                                )
                            }
                        }
                    })),
            )
            .or(callback)
            .or(warp::get()
                .and(warp::cookie("user"))
                .and_then(move |user: String| {
                    let client_codes = client_codes_c.clone();
                    async move {
                        if client_codes.lock().await.contains_key(
                            &Uuid::parse_str(&user).map_err(|_| warp::reject::not_found())?,
                        ) {
                            Ok(())
                        } else {
                            Err(warp::reject::not_found())
                        }
                    }
                })
                .untuple_one()
                .and(
                    warp::path("search")
                        .and(warp::cookie("user"))
                        .and(warp::query::<Search>())
                        .and_then(move |user: String, query: Search| {
                            let client_codes = client_codes_b.clone();
                            let client_instances = client_instances_b.clone();
                            async move {
                                let users = client_codes.lock().await;
                                let user = users.get(&Uuid::parse_str(&user).unwrap()).unwrap();
                                let clients = client_instances.lock().await;
                                let client = clients.get(&*user).unwrap();
                                let results = client
                                    .search(&query.q, SearchType::Track, 20, 0, None, None)
                                    .await
                                    .unwrap();
                                if let SearchResult::Tracks(page) = results {
                                    let data = page
                                        .items
                                        .into_iter()
                                        .map(|track| {
                                            format!(
                                                "{}+{}+{}",
                                                track.name,
                                                track.id.unwrap(),
                                                track
                                                    .album
                                                    .images
                                                    .get(0)
                                                    .map(|image| image.url.clone())
                                                    .unwrap_or("".to_owned())
                                            )
                                        })
                                        .collect::<Vec<String>>()
                                        .join("^");
                                    Ok::<_, std::convert::Infallible>(data)
                                } else {
                                    panic!()
                                }
                            }
                        })
                        .or(warp::any()
                            .map(|| warp::reply::html(include_str!("../static/index.html")))),
                ))
            .or(warp::get().and_then(move || {
                let oauth = oauth_a.clone();
                async move {
                    Ok::<_, std::convert::Infallible>(warp::redirect::temporary(
                        oauth
                            .lock()
                            .await
                            .get_authorize_url(None, None)
                            .parse::<Uri>()
                            .unwrap(),
                    ))
                }
            })),
    )
    .run(([127, 0, 0, 1], 8080))
    .await
}
