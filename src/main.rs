use chrono::{DateTime, Utc};
use clap::{Parser, ValueEnum};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    Ack,
    Nack,
}

#[derive(Parser)]
#[command(name = "ackamoto")]
#[command(about = "Track Bitcoin Core ACKs and NACKs")]
struct Args {
    #[arg(long, value_enum, default_value_t = Mode::Ack)]
    mode: Mode,
}

#[derive(Debug, Deserialize)]
struct PullRequest {
    number: u32,
    title: String,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct Comment {
    body: String,
    created_at: DateTime<Utc>,
    html_url: String,
    user: User,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
    html_url: String,
}

#[derive(Debug, Serialize)]
struct Ack {
    pr_number: u32,
    pr_title: String,
    pr_url: String,
    commenter: String,
    commenter_url: String,
    comment_url: String,
    date: DateTime<Utc>,
    comment_snippet: String,
    ack_type: String,
}

fn create_headers(token: Option<String>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("ackamoto-bot"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github.v3+json"),
    );

    if let Some(token) = token {
        if let Ok(auth_value) = HeaderValue::from_str(&format!("Bearer {}", token)) {
            headers.insert(AUTHORIZATION, auth_value);
        }
    }

    headers
}

fn extract_ack_type(body: &str, mode: &Mode) -> Option<String> {
    let lower_body = body.to_lowercase();

    match mode {
        Mode::Ack => {
            // Check for standalone ACK patterns (with word boundaries)
            if lower_body.contains("concept ack") {
                return Some("Concept ACK".to_string());
            }
            if lower_body.contains("utack") {
                return Some("utACK".to_string());
            }
            if lower_body.contains("tested ack") {
                return Some("Tested ACK".to_string());
            }
            if lower_body.contains("code review ack") {
                return Some("Code Review ACK".to_string());
            }

            // For regular ACK, ensure it's a standalone word
            let words: Vec<&str> = lower_body.split_whitespace().collect();
            for word in words {
                // Remove common punctuation from the word
                let clean_word = word.trim_matches(|c: char| !c.is_alphanumeric());
                if clean_word == "ack" {
                    return Some("ACK".to_string());
                }
            }
        }
        Mode::Nack => {
            // Check for NACK patterns
            if lower_body.contains("concept nack") {
                return Some("Concept NACK".to_string());
            }
            if lower_body.contains("strong nack") {
                return Some("Strong NACK".to_string());
            }
            if lower_body.contains("weak nack") {
                return Some("Weak NACK".to_string());
            }

            // For regular NACK, ensure it's a standalone word
            let words: Vec<&str> = lower_body.split_whitespace().collect();
            for word in words {
                // Remove common punctuation from the word
                let clean_word = word.trim_matches(|c: char| !c.is_alphanumeric());
                if clean_word == "nack" {
                    return Some("NACK".to_string());
                }
            }
        }
    }

    None
}

fn truncate_comment(body: &str, max_length: usize) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut result = String::new();

    for line in lines {
        if result.len() + line.len() > max_length {
            result.push_str("...");
            break;
        }
        result.push_str(line);
        result.push('\n');
    }

    result.trim().to_string()
}

async fn fetch_pull_requests(
    client: &reqwest::Client,
    headers: &HeaderMap,
) -> Result<Vec<PullRequest>, Box<dyn std::error::Error>> {
    let mut all_prs = Vec::new();
    let mut page = 1;

    loop {
        let url = format!(
            "https://api.github.com/repos/bitcoin/bitcoin/pulls?state=all&per_page=100&page={}",
            page
        );

        let response = client.get(&url).headers(headers.clone()).send().await?;

        if !response.status().is_success() {
            eprintln!("Failed to fetch PRs: {}", response.status());
            break;
        }

        let prs: Vec<PullRequest> = response.json().await?;
        if prs.is_empty() {
            break;
        }

        all_prs.extend(prs);

        // Fetch 5 pages (500 PRs) to stay well under rate limits
        // 5 pages + 500 comment fetches = ~505 requests (well under 5000/hour limit)
        if page >= 5 {
            break;
        }

        page += 1;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    Ok(all_prs)
}

async fn fetch_comments_for_pr(
    client: &reqwest::Client,
    headers: &HeaderMap,
    pr_number: u32,
) -> Result<Vec<Comment>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.github.com/repos/bitcoin/bitcoin/issues/{}/comments?per_page=100",
        pr_number
    );

    let response = client.get(&url).headers(headers.clone()).send().await?;

    if !response.status().is_success() {
        eprintln!(
            "Failed to fetch comments for PR {}: {}",
            pr_number,
            response.status()
        );
        return Ok(Vec::new());
    }

    let comments: Vec<Comment> = response.json().await?;
    Ok(comments)
}

fn generate_error_html(error_message: &str, mode: &Mode) -> String {
    let (site_name, site_type, _site_title) = match mode {
        Mode::Ack => ("ackamoto", "ACK", "ACKamoto"),
        Mode::Nack => ("nackamoto", "NACK", "NACKamoto"),
    };
    
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Bitcoin Core {}s - {}.com</title>
    <link rel="icon" type="image/png" href="images/{}-logo.png">
    <link href="https://fonts.googleapis.com/css2?family=Roboto:wght@100;400&family=Roboto+Mono:wght@100;400&family=Cormorant+Garamond:wght@300;400&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-color: #f8f5ea;
            --text-color: #222;
        }}
        
        @media (prefers-color-scheme: dark) {{
            :root {{
                --bg-color: #000;
                --text-color: #fff;
            }}
        }}
        
        body {{
            font-family: 'Roboto Mono', monospace;
            font-weight: 400;
            line-height: 1.2;
            color: var(--text-color);
            max-width: 900px;
            margin: 0 auto;
            padding: 2rem;
            background: var(--bg-color);
            font-size: 1rem;
            letter-spacing: 0;
        }}
        h1 {{
            font-size: 4rem;
            font-weight: 400;
            color: var(--text-color);
            text-align: center;
            margin-bottom: 1rem;
            letter-spacing: 0.01em;
            font-family: 'Roboto Mono', monospace;
            font-weight: 400;
        }}
        .error-message {{
            font-size: 1.4rem;
            color: var(--text-color);
            text-align: center;
            margin: 2rem 0;
            padding: 2rem;
            border: 2px solid var(--text-color);
            background: transparent;
        }}
    </style>
</head>
<body>
    <h1>Bitcoin Core {}s</h1>
    <div class="error-message">
        {}<br><br>
        The site will automatically retry when GitHub Actions runs every 2 hours.
    </div>
</body>
</html>"#,
        site_type, site_name, site_name, site_type, error_message
    )
}

fn format_date(date: &DateTime<Utc>) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn generate_html(acks: &[Ack], mode: &Mode) -> String {
    let now = Utc::now();
    let (site_name, site_type, _site_title) = match mode {
        Mode::Ack => ("ackamoto", "ACK", "ACKamoto"),
        Mode::Nack => ("nackamoto", "NACK", "NACKamoto"),
    };
    
    // Calculate date range if we have ACKs
    let _date_range_text = if !acks.is_empty() {
        let oldest_date = acks.iter().map(|ack| &ack.date).min().unwrap();
        let newest_date = acks.iter().map(|ack| &ack.date).max().unwrap();
        format!(
            "{} to {}",
            oldest_date.format("%Y-%m-%d"),
            newest_date.format("%Y-%m-%d")
        )
    } else {
        // When no ACKs found, show an approximate date range for the PRs we searched
        let end_date = Utc::now();
        let start_date = end_date - chrono::Duration::days(7);
        format!(
            "{} to {}",
            start_date.format("%Y-%m-%d"),
            end_date.format("%Y-%m-%d")
        )
    };
    
    // If no ACKs, generate a simple empty page
    if acks.is_empty() {
        return format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Bitcoin Core {}s - {}.com</title>
    <link rel="icon" type="image/png" href="images/{}-logo.png">
    <link href="https://fonts.googleapis.com/css2?family=Roboto:wght@100;400&family=Roboto+Mono:wght@100;400&family=Cormorant+Garamond:wght@300;400&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-color: #f8f5ea;
            --text-color: #222;
        }}
        
        @media (prefers-color-scheme: dark) {{
            :root {{
                --bg-color: #000;
                --text-color: #fff;
            }}
        }}
        
        body {{
            font-family: 'Roboto Mono', monospace;
            font-weight: 400;
            line-height: 1.2;
            color: var(--text-color);
            max-width: 900px;
            margin: 0 auto;
            padding: 2rem;
            background: var(--bg-color);
            font-size: 1rem;
            letter-spacing: 0;
        }}
        .title-section {{
            text-align: center;
            margin-bottom: 8rem;
        }}
        .logo {{
            height: 16rem;
            width: auto;
            display: block;
            margin: 0 auto;
        }}
        .logo-light {{
            display: block;
        }}
        .logo-dark {{
            display: none;
        }}
        @media (prefers-color-scheme: dark) {{
            .logo-light {{
                display: none;
            }}
            .logo-dark {{
                display: block;
            }}
        }}
        .last-updated {{
            color: #888;
            font-size: 1rem;
            margin-bottom: 3rem;
            margin-top: 0;
            text-align: left;
            font-family: 'Cormorant Garamond', serif;
            font-weight: 300;
            letter-spacing: 0;
            transform: scaleX(0.85);
        }}
        .date-range {{
            color: #888;
            font-size: 0.9rem;
            margin-bottom: 3rem;
            margin-top: 0;
            text-align: left;
            font-family: 'Roboto Mono', monospace;
            font-weight: 400;
            letter-spacing: 0;
        }}
        @media (max-width: 768px) {{
            body {{
                padding: 1rem;
            }}
            .logo {{
                height: 8rem;
            }}
        }}
    </style>
</head>
<body>
    <div class="title-section">
        <img src="images/{}-logo.png" alt="{}" class="logo logo-light">
        <img src="images/{}-logo-dark.png" alt="{}" class="logo logo-dark">
    </div>
    <p class="last-updated">Last updated at {} UTC</p>
</body>
</html>"#,
            site_type, site_name, site_name, site_name, _site_title, site_name, _site_title, now.format("%Y-%m-%d %H:%M")
        );
    }

    // Group ACKs by date
    let mut acks_by_date: std::collections::HashMap<String, Vec<&Ack>> = std::collections::HashMap::new();
    for ack in acks {
        let date_key = format_date(&ack.date);
        acks_by_date.entry(date_key).or_insert_with(Vec::new).push(ack);
    }
    
    // Filter out empty date groups and sort dates chronologically (most recent first)
    let mut sorted_dates: Vec<_> = acks_by_date.iter()
        .filter(|(_, acks)| !acks.is_empty())
        .map(|(date, _)| date.clone())
        .collect();
    sorted_dates.sort_by(|a, b| b.cmp(a)); // Sort dates in reverse order (newest first)

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Bitcoin Core {}s - {}.com</title>
    <link rel="icon" type="image/png" href="images/{}-logo.png">
    <link href="https://fonts.googleapis.com/css2?family=Roboto:wght@100;400&family=Roboto+Mono:wght@100;400&family=Cormorant+Garamond:wght@300;400&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-color: #f8f5ea;
            --text-color: #222;
            --border-color: #e5e5e5;
        }}
        
        @media (prefers-color-scheme: dark) {{
            :root {{
                --bg-color: #000;
                --text-color: #fff;
                --border-color: #333;
            }}
        }}
        
        body {{
            font-family: 'Roboto Mono', monospace;
            font-weight: 400;
            line-height: 1.2;
            color: var(--text-color);
            max-width: 900px;
            margin: 0 auto;
            padding: 2rem;
            background: var(--bg-color);
            font-size: 1rem;
            letter-spacing: 0;
        }}
        .title-section {{
            text-align: center;
            margin-bottom: 8rem;
        }}
        .logo {{
            height: 16rem;
            width: auto;
            display: block;
            margin: 0 auto;
        }}
        .logo-light {{
            display: block;
        }}
        .logo-dark {{
            display: none;
        }}
        @media (prefers-color-scheme: dark) {{
            .logo-light {{
                display: none;
            }}
            .logo-dark {{
                display: block;
            }}
        }}
        .date-header {{
            font-family: 'Cormorant Garamond', serif;
            font-weight: 300;
            font-size: 1rem;
            color: #888;
            text-align: left;
            margin: 3rem 0 4rem 0;
            letter-spacing: 0;
            transform: scaleX(0.85);
        }}
        .last-updated {{
            color: #888;
            font-size: 1rem;
            margin-bottom: 3rem;
            margin-top: 0;
            text-align: left;
            font-family: 'Cormorant Garamond', serif;
            font-weight: 300;
            letter-spacing: 0;
            transform: scaleX(0.85);
        }}
        .date-range {{
            color: #888;
            font-size: 1rem;
            margin-bottom: 3rem;
            margin-top: 0;
            text-align: left;
            font-family: 'Cormorant Garamond', serif;
            font-weight: 300;
            letter-spacing: 0;
            transform: scaleX(0.85);
        }}
        .acks-container {{
            margin-top: 1rem;
        }}
        .ack-entry {{
            display: flex;
            flex-direction: column;
            gap: 1rem;
            margin-bottom: 4rem;
        }}
        a {{
            color: var(--text-color);
            text-decoration: underline;
            text-underline-offset: 0.3em;
        }}
        a:hover {{
            color: var(--text-color);
        }}
        .pr-title {{
            word-wrap: break-word;
            line-height: 1rem;
        }}
        a.pr-number {{
            font-size: 1rem;
            font-weight: 400;
        }}
        .pr-title, .ack-type {{
            font-size: 1rem;
            font-weight: 400;
        }}
        a.commenter {{
            font-size: 1rem;
            font-weight: 400;
        }}
        .ack-type {{
            display: inline-block;
            padding: 0.5rem;
            border: 2px solid var(--text-color);
            width: fit-content;
        }}
        @media (max-width: 768px) {{
            body {{
                padding: 1rem;
            }}
            .logo {{
                height: 8rem;
            }}
        }}
    </style>
</head>
<body>
    <div class="title-section">
        <img src="images/{}-logo.png" alt="{}" class="logo logo-light">
        <img src="images/{}-logo-dark.png" alt="{}" class="logo logo-dark">
    </div>
    <p class="last-updated">Last updated at {}</p>
"#,
        site_type, site_name, site_name, site_name, _site_title, site_name, _site_title, now.format("%Y-%m-%d %H:%M UTC")
    ) + &sorted_dates
        .iter()
        .map(|date| {
            let date_acks = &acks_by_date[date];
            let date_header = format!(
                r#"    
    <h2 class="date-header">{}</h2>
    
    <div class="acks-container">
"#,
                date.to_uppercase()
            );
            
            let rows = date_acks
                .iter()
                .map(|ack| {
                    format!(
                        r#"        <div class="ack-entry">
            <a href="{}" target="_blank" class="pr-number">#{}</a>
            <div class="pr-title" title="{}">{}</div>
            <div class="ack-type">{}</div>
            <a href="{}" target="_blank" class="commenter">{}</a>
        </div>
"#,
                        ack.pr_url,
                        ack.pr_number,
                        ack.pr_title.replace('"', "&quot;"),
                        ack.pr_title.replace('<', "&lt;").replace('>', "&gt;"),
                        ack.ack_type,
                        ack.comment_url,
                        ack.commenter
                    )
                })
                .collect::<String>();
            
            date_header + &rows
        })
        .collect::<String>()
        + r#"    </div>
</body>
</html>"#
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let token = env::var("GITHUB_TOKEN").ok();

    let client = reqwest::Client::new();
    let headers = create_headers(token);

    println!("Fetching pull requests...");
    let prs = match fetch_pull_requests(&client, &headers).await {
        Ok(prs) => {
            println!("Found {} pull requests", prs.len());
            prs
        }
        Err(e) => {
            println!("Failed to fetch PRs: {}", e);
            // Return empty HTML with error message
            let html = generate_error_html(
                "Unable to fetch data from GitHub API. This may be due to rate limiting.",
                &args.mode,
            );
            fs::write("index.html", html)?;
            return Ok(());
        }
    };

    let mut all_acks = Vec::new();

    // Check if we have a GitHub token to determine rate limits
    let has_token = env::var("GITHUB_TOKEN").is_ok();
    
    let prs_limit = if has_token {
        250  // With token, check 250 PRs for both ACKs and NACKs
    } else {
        50   // Without token, check 50 PRs for both ACKs and NACKs
    };
    
    if !has_token {
        println!("Warning: No GITHUB_TOKEN found. API requests will be limited.");
    }
    
    let prs_to_process = prs.iter().take(prs_limit).collect::<Vec<_>>();

    for (i, pr) in prs_to_process.iter().enumerate() {
        if i % 10 == 0 {
            println!("Processing PR {}/{}", i + 1, prs_to_process.len());
        }

        let comments = fetch_comments_for_pr(&client, &headers, pr.number).await?;

        for comment in comments {
            // Skip bot comments
            let username_lower = comment.user.login.to_lowercase();
            if username_lower.contains("bot") || username_lower == "bitcoin-core-ci" {
                continue;
            }

            if let Some(ack_type) = extract_ack_type(&comment.body, &args.mode) {
                let ack = Ack {
                    pr_number: pr.number,
                    pr_title: pr.title.clone(),
                    pr_url: pr.html_url.clone(),
                    commenter: comment.user.login.clone(),
                    commenter_url: comment.user.html_url.clone(),
                    comment_url: comment.html_url.clone(),
                    date: comment.created_at,
                    comment_snippet: truncate_comment(&comment.body, 200),
                    ack_type,
                };
                all_acks.push(ack);
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    all_acks.sort_by(|a, b| b.date.cmp(&a.date));

    println!("Found {} ACKs total", all_acks.len());

    let html = generate_html(&all_acks, &args.mode);
    fs::write("index.html", html)?;
    println!("Generated index.html");

    Ok(())
}
