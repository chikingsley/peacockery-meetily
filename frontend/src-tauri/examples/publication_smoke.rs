//! Explicit live upload of synthetic test audio through the production app pipeline.
//! cargo run -p peacockery-meetily --example publication_smoke -- <scratch-folder> --allow-public
use app_lib::{
    database::{
        manager::DatabaseManager,
        repositories::meeting::{MeetingsRepository, NewMeeting},
    },
    publication,
};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[2] != "--allow-public" {
        return Err("Provide a scratch folder with synthetic audio.mp4 and --allow-public".into());
    }
    let dir = std::path::Path::new(&args[1]);
    let db = DatabaseManager::open(&dir.join("publication-smoke.sqlite")).await?;
    if MeetingsRepository::get(db.pool(), "publication-smoke")
        .await?
        .is_none()
    {
        MeetingsRepository::create(
            db.pool(),
            &NewMeeting {
                id: "publication-smoke",
                title: "Meetily publishing verification - temporary synthetic audio",
                folder_path: dir.to_str().ok_or("Invalid path")?,
                status: "stopped",
            },
        )
        .await?;
        publication::enqueue(db.pool(), "publication-smoke").await?;
    }
    publication::tick(db.pool()).await?;
    let (state, upload_id, error): (String, Option<String>, Option<String>) = sqlx::query_as("SELECT state,upload_id,error FROM meeting_publications WHERE meeting_id='publication-smoke'").fetch_one(db.pool()).await?;
    println!(
        "{}",
        serde_json::json!({"state":state,"upload_id":upload_id,"error":error})
    );
    db.cleanup().await;
    Ok(())
}
