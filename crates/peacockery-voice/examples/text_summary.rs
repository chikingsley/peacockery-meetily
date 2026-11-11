//! Summarize a transcript text file through `/v1/text`.
//!
//! ```text
//! PEACOCKERY_VOICE_LAB_API_KEY=… cargo run -p peacockery-voice --example text_summary -- transcript.txt [model]
//! ```

use peacockery_voice::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: text_summary <transcript.txt> [model]")?;
    let model = args
        .next()
        .unwrap_or_else(|| "google-gemini-3.8-flash".into());
    let transcript = std::fs::read_to_string(path)?;

    let key = std::env::var("PEACOCKERY_VOICE_LAB_API_KEY")?;
    let base =
        std::env::var("PEACOCKERY_VOICE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
    let client = VoiceClient::new(ClientConfig::new(base, key))?;

    let mut request = TextRequest::new(
        model,
        vec![
            Message::system("Summarize this call transcript as short meeting notes: key points, decisions, action items with owners."),
            Message::user(transcript),
        ],
    );
    request.max_output_tokens = Some(1500);
    request.routing = Some(Routing {
        fallback_models: vec![
            "deepseek-deepseek-v4-flash".into(),
            "google-gemini-3.1-flash-lite".into(),
        ],
        attempt_timeout_ms: None,
    });
    match client.generate(&request).await {
        Ok(r) => {
            println!("{}", r.text);
            eprintln!(
                "model {} ({:?}), {:?} tokens, {:?} ms",
                r.model, r.provider, r.usage.total_tokens, r.provider_latency_ms
            );
        }
        Err(Error::Truncated(partial)) => {
            println!("{}", partial.text);
            eprintln!("truncated at maxOutputTokens");
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}
