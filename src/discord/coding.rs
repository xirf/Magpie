use serenity::builder::{
    CreateCommand, CreateCommandOption, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse,
};
use serenity::model::application::{CommandDataOptionValue, CommandInteraction, CommandOptionType};
use serenity::prelude::Context;

use super::state::Handler;
use crate::config::Settings;

pub fn command(project_aliases: &[String]) -> CreateCommand {
    let mut project_option = CreateCommandOption::new(
        CommandOptionType::String,
        "project",
        "Select a configured project",
    );
    if project_aliases.len() <= 25 {
        for alias in project_aliases {
            if !alias.is_empty() && alias.chars().count() <= 100 {
                project_option = project_option.add_string_choice(alias.as_str(), alias.as_str());
            }
        }
    }
    CreateCommand::new("code")
        .description("Queue and manage coding tasks (configured administrators only)")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "action", "Operation")
                .required(true)
                .add_string_choice("Task", "task")
                .add_string_choice("Status", "status")
                .add_string_choice("Cancel", "cancel"),
        )
        .add_option(project_option)
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "task",
                "What the coding agent should implement",
            )
            .max_length(6000),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::Integer, "id", "Task ID").min_int_value(1),
        )
}

pub async fn handle(
    handler: &Handler,
    ctx: &Context,
    cmd: &CommandInteraction,
    settings: &Settings,
) {
    let user = cmd.user.id.to_string();
    let channel = cmd.channel_id.to_string();
    if !crate::coding::authorized(settings, &user, &channel) {
        let _ = cmd.create_response(&ctx.http,CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content("Coding requires an explicitly configured administrator and allowed channel.")
                .ephemeral(true))).await;
        return;
    }
    let Some(service) = handler.coding.as_ref() else {
        let _ = cmd
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(
                            "Coding is unavailable. Check server configuration and restart Magpie.",
                        )
                        .ephemeral(true),
                ),
            )
            .await;
        return;
    };
    if cmd.defer_ephemeral(&ctx.http).await.is_err() {
        return;
    }
    let string = |name: &str| {
        cmd.data
            .options
            .iter()
            .find(|o| o.name == name)
            .and_then(|o| o.value.as_str())
    };
    let id = cmd
        .data
        .options
        .iter()
        .find(|o| o.name == "id")
        .and_then(|o| {
            if let CommandDataOptionValue::Integer(id) = &o.value {
                Some(*id)
            } else {
                None
            }
        });
    let result: Result<String,String> = match string("action") {
        Some("task") => {
            match (string("project"),string("task")) {
                (Some(project),Some(task)) if service.config.projects.contains_key(project)
                    && !task.trim().is_empty() && task.chars().count() <= 6000 => {
                    service.store.enqueue(project,task,&channel,&user)
                        .map(|id|format!("Queued coding task #{id}. Use `/code action:status id:{id}` to follow it."))
                        .map_err(|_| "Cannot enqueue task; check queue capacity and disk availability".into())
                }
                (Some(project), _) if !service.config.projects.contains_key(project) => {
                    Err(format!("Unknown project `{project}`. Select one of: {}. The project field is an existing alias, not a new project name.",
                        service.config.projects.keys().cloned().collect::<Vec<_>>().join(", ")))
                }
                (None, _) => Err(format!("Select a project: {}.",
                    service.config.projects.keys().cloned().collect::<Vec<_>>().join(", "))),
                _ => Err("Provide a non-empty task description (maximum 6000 characters). The id option is only needed for status or cancel.".into()),
            }
        }
        Some("status") => {
            let jobs = if let Some(id) = id {
                service.store.get(id).map(|job|job.filter(|j|j.channel == channel).into_iter().collect::<Vec<_>>())
            } else { service.store.recent(&channel) };
            jobs.map(|jobs| {
                if jobs.is_empty() { return "No matching coding tasks in this channel.".into(); }
                jobs.iter().map(|j|format!("#{} [{}] {} — {}",j.id,j.status,j.project,j.detail)).collect::<Vec<_>>().join("\n")
            }).map_err(|_| "Cannot read coding task status".into())
        }
        Some("cancel") => {
            if let Some(id) = id {
                service.cancel(id,&channel).await.map(|changed| {
                    if changed { format!("Cancellation requested for task #{id}.") }
                    else { "No active matching task in this channel.".into() }
                })
            } else { Err("Provide the task ID to cancel.".into()) }
        }
        _ => Err("Choose task, status, or cancel.".into()),
    };
    let text = result.unwrap_or_else(|error| error);
    let _ = cmd
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content(text.chars().take(1900).collect::<String>()),
        )
        .await;
}
