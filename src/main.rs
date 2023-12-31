use clap::Parser;
use dirs;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::time::Duration;
use std::{
    env,
    path::PathBuf,
    env::current_exe,
    fs::{self},
    io::{Error, Read},
};
use chrono::Utc;
use indicatif::{ProgressBar, ProgressStyle};

const MAX_TOKENS: i64 = 2000;
const DEFAULT_TIMEOUT_SECS: u64 = 120;


#[derive(Serialize, Deserialize, Debug)]
struct Log {
    timestamp: String,
    role: String,
    content: String,
    tokens: i64,
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    role: String,
    content: String,
}
#[derive(Debug, Deserialize, Serialize)]
struct OpenAIRequest {
    #[serde(rename = "model")]
    model: String,
    #[serde(rename = "messages")]
    messages: Vec<Message>,
}

fn create_message(role: String, content: String) -> Message {
    Message {
        role,
        content,
    }
}

fn create_log(role: String, content: String, tokens: i64) -> Log {
    Log {
        timestamp: Utc::now().to_rfc3339(),
        role,
        content,
        tokens,
    }
}


fn main() -> Result<(), Error> {
    
    let dotenv_path = match current_exe() {
        Ok(mut path) => {
            path.pop(); // This will remove the binary name from the path.
            path.push(".env"); // This will append the .env file name to the path.
            path
        },
        Err(e) => {
            eprintln!("Failed to get current binary path: {}", e);
            PathBuf::from(".env") // Fallback to looking for .env in the current directory.
        },
    };
    
    dotenv::from_path(dotenv_path.as_path()).ok();
    
    let args = CliArgs::parse();

    // get OPENAI_API_KEY from environment variable
    let key = "OPENAI_API_KEY";
    let openai_api_key = env::var(key).expect(&format!("{} not set", key));
    let openai_api_base = env::var("OPENAI_API_BASE").unwrap_or_else(|_| String::from("https://api.openai.com/v1/chat/completions/"));
    // get the prompt from the user
    let prompt = args.prompt.join(" ");

    // Get the model from the CLI argument, environment variable, or use the default value
    let model = args
        .model
        .or_else(|| env::var("CHATGPT_CLI_MODEL").ok())
        .unwrap_or_else(|| "gpt-3.5-turbo".to_string());


    // load the chatlog for this terminal window
    let chatlog_path = dirs::home_dir()
    .expect("Failed to get home directory")
    .join(".ask/ask_log.json");


    fs::create_dir_all(chatlog_path.parent().unwrap())?;

    let mut file = OpenOptions::new()
        .create(true) // create the file if it doesn't exist
        .append(true) // don't overwrite the contents
        .read(true)
        .open(&chatlog_path)
        .unwrap();

    let mut chatlog_text = String::new();
    file.read_to_string(&mut chatlog_text)?;

    // get the messages from the chatlog. limit the total number of tokens to 3000
    let mut total_tokens: i64 = 0;
    let mut messages: Vec<Message> = vec![];
    let mut chatlog: Vec<Log> = vec![];

    if !chatlog_text.is_empty() {
        chatlog = serde_json::from_str(&chatlog_text)?;
        for log in chatlog.iter().rev() {
            if total_tokens + log.tokens > MAX_TOKENS {
                continue;
            }

            total_tokens += log.tokens;
            messages.push(create_message(log.role.clone(), log.content.clone()));

        }
    }

    messages = messages.into_iter().rev().collect();

    messages.push(create_message("user".to_string(), prompt.clone()));



    let client = Client::new();
    let data = OpenAIRequest {     // send the POST request to OpenAI
        model: model.to_string(),
        messages,
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", openai_api_key).parse().unwrap(),
    );
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
    let json_data = serde_json::to_string(&data)?;
    let timeout_secs = env::var("CHATGPT_CLI_REQUEST_TIMEOUT_SECS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS); // default value of 120 seconds
    // Create a spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::default_spinner());

    // Start the spinner
    spinner.enable_steady_tick(Duration::from_millis(100));

    let response = client
        .post(&openai_api_base)
        .timeout(Duration::from_secs(timeout_secs))
        .headers(headers)
        .body(json_data)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();

    // Stop the spinner
    spinner.finish_and_clear();

    // if the response is an error, print it and exit
    match response["error"].as_object() {
        None => response["error"].clone(),
        Some(_) => {
            println!(
                "Received an error from OpenAI: {}",
                response["error"]["message"].as_str().unwrap()
            );
            return Ok(());
        }
    };

    let prompt_tokens = response["usage"]["prompt_tokens"].as_i64().unwrap();
    let answer_tokens = response["usage"]["completion_tokens"].as_i64().unwrap();
    let answer = response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();

    // Show the response from OpenAI
    println!("{}", answer);

    // save the new messages to the chatlog
    chatlog.push(create_log("user".to_string(), prompt, prompt_tokens));
    chatlog.push(create_log("assistant".to_string(), answer.to_string(), answer_tokens));


    // write the chatlog to disk
    let chatlog_text = serde_json::to_string(&chatlog)?;
    fs::write(&chatlog_path, chatlog_text)?;

    Ok(())
}

// get version from Cargo.toml
#[derive(Parser, Debug)]
#[clap(version = env!("CARGO_PKG_VERSION"))]
struct CliArgs {
    /// The prompt to send to ChatGPT
    #[clap(name = "prompt")]
    prompt: Vec<String>,

    /// The ChatGPT model to use (default: gpt-3.5-turbo)
    #[clap(short, long)]
    model: Option<String>,
}
