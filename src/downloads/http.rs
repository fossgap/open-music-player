use super::*;
use reqwest::{StatusCode, header};

pub(super) async fn download(
    task: &Task,
    resolved: &youtube_music::Resolved,
    progress: &mut Progress<'_>,
) -> Result<Option<(NamedTempFile, String)>, String> {
    let mut partial = task.partial.lock().unwrap().take();
    let result = transfer(task, resolved, progress, &mut partial).await;
    if result.is_err() && task.control.value() != DISCARD {
        if let Some(partial) = partial.filter(|p| p.bytes > 0) {
            let _ = progress
                .tx
                .send((task.id, Update::Progress(partial.bytes, partial.total, 0.0)));
            *task.partial.lock().unwrap() = Some(partial);
        }
    }
    result
}

async fn transfer(
    task: &Task,
    resolved: &youtube_music::Resolved,
    progress: &mut Progress<'_>,
    partial: &mut Option<Partial>,
) -> Result<Option<(NamedTempFile, String)>, String> {
    if partial.as_ref().is_some_and(|p| {
        !p.file.path().is_file() || p.file.as_file().metadata().is_ok_and(|m| m.len() < p.bytes)
    }) {
        partial.take();
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|_| "Could not initialize downloads.")?;
    // Strong validators avoid combining bytes from two different representations.
    // Sources without one restart safely; signed URLs are resolved again by execute.
    let offset = partial
        .as_ref()
        .filter(|p| p.validator.is_some())
        .map_or(0, |p| p.bytes);
    let mut offset = offset;
    let mut response = loop {
        let mut request = client
            .get(&resolved.uri)
            .header(header::ACCEPT_ENCODING, "identity")
            .header(
                header::USER_AGENT,
                concat!(
                    "OpenMusicPlayer/",
                    env!("CARGO_PKG_VERSION"),
                    " (https://github.com/fossgap/open-music-player)"
                ),
            );
        for (key, value) in &resolved.headers {
            request = request.header(key, value);
        }
        if offset > 0 {
            request = request
                .header(header::RANGE, format!("bytes={offset}-"))
                .header(
                    header::IF_RANGE,
                    partial.as_ref().unwrap().validator.as_deref().unwrap(),
                );
        }
        let response = tokio::select! {
            _ = interrupted(&task.control) => return Err("Cancelled".into()),
            result = tokio::time::timeout(Duration::from_secs(30), request.send()) => result.map_err(|_| "The download timed out. Try again.")?.map_err(|_| "Could not connect to this source. Try again.")?,
        };
        let range = response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(content_range);
        let valid_range = range.is_some_and(|(start, end, total)| {
            start == offset
                && end >= start
                && end < total
                && response
                    .content_length()
                    .is_none_or(|n| n == end - start + 1)
                && partial
                    .as_ref()
                    .and_then(|p| p.total)
                    .is_none_or(|old| old == total)
        });
        let same_validator = offset == 0
            || validator(&response).as_ref() == partial.as_ref().and_then(|p| p.validator.as_ref());
        if offset > 0
            && (response.status() == StatusCode::RANGE_NOT_SATISFIABLE
                || (response.status() == StatusCode::PARTIAL_CONTENT
                    && (!valid_range || !same_validator)))
        {
            // Retry once without a range if the remote representation changed.
            offset = 0;
            partial.take();
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "The source rejected this download (HTTP {}).",
                response.status().as_u16()
            ));
        }
        if response.status() == StatusCode::PARTIAL_CONTENT && !valid_range {
            return Err("The source returned an invalid download range. Try again.".into());
        }
        if response.status() != StatusCode::PARTIAL_CONTENT {
            offset = 0;
        }
        break response;
    };
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_lowercase();
    let path = response.url().path().to_lowercase();
    if content_type.contains("mpegurl")
        || content_type.contains("dash+xml")
        || path.ends_with(".m3u8")
        || path.ends_with(".mpd")
    {
        partial.take();
        return Ok(None);
    }
    if content_type.contains("html")
        || content_type.contains("json")
        || content_type.starts_with("text/")
    {
        return Err("This address did not return audio. Use a direct audio link.".into());
    }
    let total = if response.status() == StatusCode::PARTIAL_CONTENT {
        response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(content_range)
            .map(|(_, _, total)| total)
    } else {
        response.content_length()
    };
    if offset == 0 {
        partial.take();
        *partial = Some(Partial {
            file: tempfile::Builder::new()
                .prefix(".open-music-")
                .suffix(".part")
                .tempfile_in(&task.directory)
                .map_err(|e| e.to_string())?,
            bytes: 0,
            total,
            validator: validator(&response),
        });
    }
    let saved = partial.as_mut().unwrap();
    saved.total = total;
    // Drop any bytes from a write that failed halfway through on the last attempt.
    saved
        .file
        .as_file_mut()
        .set_len(saved.bytes)
        .map_err(|e| e.to_string())?;
    saved
        .file
        .as_file_mut()
        .seek(SeekFrom::End(0))
        .map_err(|e| e.to_string())?;
    progress.status(Status::Running);
    progress.send(saved.bytes, total, 0.0);
    loop {
        let chunk = tokio::select! {
            _ = interrupted(&task.control) => return Err("Cancelled".into()),
            result = tokio::time::timeout(Duration::from_secs(30), response.chunk()) => result.map_err(|_| "The download stalled. Try again.")?.map_err(|_| "The download was interrupted. Try again.")?,
        };
        let Some(chunk) = chunk else { break };
        saved
            .file
            .write_all(&chunk)
            .map_err(|e| format!("Could not write the download. Check free space: {e}"))?;
        saved.bytes += chunk.len() as u64;
        progress.send(saved.bytes, total, 0.0);
    }
    if saved.bytes == 0 || total.is_some_and(|n| n != saved.bytes) {
        return Err("The download was incomplete. Try again.".into());
    }
    saved
        .file
        .as_file_mut()
        .rewind()
        .map_err(|e| e.to_string())?;
    let mut prefix = [0; 64];
    let count = saved.file.read(&mut prefix).map_err(|e| e.to_string())?;
    let extension = extension(&content_type, &path, &prefix[..count]).to_string();
    let _ = progress.tx.send((
        task.id,
        Update::Progress(saved.bytes, Some(saved.bytes), 0.0),
    ));
    Ok(Some((partial.take().unwrap().file, extension)))
}

fn validator(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .filter(|tag| tag.starts_with('"') && tag.ends_with('"'))
        .map(str::to_string)
}

fn content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}
