use crate::app::message::Message;
use crate::config::AppConfig;
use crate::llm::chat::{prepare_chat_request, ChatRequest, PreparedChatRequest};

pub(crate) struct RequestPreparation {
    pub(crate) conversation_id: i64,
    pub(crate) provider: String,
    pub(crate) endpoint: String,
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) supported_reasoning_efforts: Vec<String>,
    pub(crate) backend_type: String,
    pub(crate) local_api_key: Option<String>,
    pub(crate) credential_env_var: Option<String>,
    pub(crate) system_prompt: String,
    pub(crate) messages: Vec<Message>,
    pub(crate) config: AppConfig,
    pub(crate) user_request: String,
    pub(crate) is_local_provider: bool,
    pub(crate) vault_result: Option<crate::app::vault_mutation_execute::VaultMutationToolResult>,
}

pub(crate) struct PreparedLaunch {
    pub(crate) request: PreparedChatRequest,
    pub(crate) notices: Vec<String>,
    #[cfg(feature = "memory")]
    pub(crate) memory_activities: Vec<crate::memory::MemoryActivity>,
}

pub(crate) enum PreparationOutcome {
    Ready(Box<PreparedLaunch>),
    VaultMutation(crate::vault::mutation_plan::VaultMutationKind),
}

pub(crate) async fn prepare_launch(
    preparation: RequestPreparation,
) -> color_eyre::Result<PreparationOutcome> {
    let RequestPreparation {
        conversation_id,
        provider,
        endpoint,
        model,
        reasoning_effort,
        supported_reasoning_efforts,
        backend_type,
        local_api_key,
        credential_env_var,
        system_prompt,
        messages,
        config,
        user_request,
        is_local_provider,
        vault_result,
    } = preparation;

    let api_key = if is_local_provider {
        local_api_key
    } else {
        match credential_env_var {
            Some(env_var) => {
                let request =
                    crate::llm::auth::CredentialRequest::new(&provider, &env_var, &endpoint);
                crate::llm::auth::resolve_provider_credential(request)
                    .await?
                    .map(|credential| {
                        secrecy::ExposeSecret::expose_secret(credential.bearer_token()).to_owned()
                    })
            }
            None => None,
        }
    };
    if !is_local_provider
        && !provider.eq_ignore_ascii_case("Ollama")
        && backend_type.trim() != "codex"
        && api_key.is_none()
    {
        return Err(color_eyre::eyre::eyre!(
            "No API key or OAuth token found for {provider}. Open Settings > Providers or set the provider env var."
        ));
    }

    let mut runtime_system_prompt = system_prompt;
    let mut request = ChatRequest {
        provider,
        endpoint,
        model,
        reasoning_effort,
        supported_reasoning_efforts,
        backend_type,
        api_key,
        system_prompt: runtime_system_prompt.clone(),
        messages,
    };
    if let Some(result) = vault_result {
        request.messages.push(result.message(conversation_id));
        let request = prepare_chat_request(request)?;
        return Ok(PreparationOutcome::Ready(Box::new(PreparedLaunch {
            request,
            notices: Vec::new(),
            #[cfg(feature = "memory")]
            memory_activities: Vec::new(),
        })));
    }
    let skills = crate::skill_runtime::prepare(&config, &user_request, &request).await;
    if let Some(kind) = skills.vault_mutation {
        return Ok(PreparationOutcome::VaultMutation(kind));
    }
    runtime_system_prompt.push_str(&skills.context);
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
    runtime_system_prompt.push_str(&format!(
        "\n\nCurrent date and time: {now}. Use this when referencing relative dates like 'today' or 'yesterday'."
    ));
    request.messages.extend(skills.messages);
    let mut notices = skills.notices;

    match crate::search::maybe_search(&config, &user_request, false).await {
        Ok(Some(context)) => request
            .messages
            .push(crate::search::untrusted_context_message(&context)),
        Ok(None) => {}
        Err(error) => notices.push(format!("Web search unavailable: {error}")),
    }

    #[cfg(feature = "memory")]
    let mut memory_activities = Vec::new();
    #[cfg(feature = "memory")]
    if config.memory.enabled {
        memory_activities.push(crate::memory::MemoryActivity::Recalling);
        let recall_config = config.clone();
        let recall_query = user_request.clone();
        match tokio::task::spawn_blocking(move || {
            crate::memory::recall(&recall_config, &recall_query)
        })
        .await
        {
            Ok(Ok(recall)) if !recall.context.is_empty() => {
                memory_activities.clear();
                memory_activities.push(crate::memory::MemoryActivity::Recalled {
                    titles: recall.titles,
                });
                crate::memory::append_recall(&mut request.messages, &recall.context);
            }
            Ok(Ok(_)) => memory_activities.clear(),
            Ok(Err(error)) => {
                memory_activities.clear();
                memory_activities.push(crate::memory::MemoryActivity::Failed {
                    message: error.to_string(),
                });
            }
            Err(error) => {
                memory_activities.clear();
                memory_activities.push(crate::memory::MemoryActivity::Failed {
                    message: error.to_string(),
                });
            }
        }

        let operation_config = config.clone();
        let operation_request = user_request.clone();
        match tokio::task::spawn_blocking(move || {
            crate::memory::run_skill_operation(&operation_config, &operation_request)
        })
        .await
        {
            Ok(Ok(Some(operation))) => {
                if !operation.context.is_empty() {
                    request.messages.push(Message::new(
                        conversation_id,
                        "user".to_string(),
                        operation.context,
                    ));
                }
                if let Some(activity) = operation.activity {
                    memory_activities.push(activity);
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => memory_activities.push(crate::memory::MemoryActivity::Failed {
                message: error.to_string(),
            }),
            Err(error) => memory_activities.push(crate::memory::MemoryActivity::Failed {
                message: error.to_string(),
            }),
        }
        if config.memory.auto_capture {
            runtime_system_prompt.push_str(crate::memory::AUTO_CAPTURE_POLICY);
        }
    }
    request.system_prompt = runtime_system_prompt;
    let request = prepare_chat_request(request)?;
    Ok(PreparationOutcome::Ready(Box::new(PreparedLaunch {
        request,
        notices,
        #[cfg(feature = "memory")]
        memory_activities,
    })))
}
