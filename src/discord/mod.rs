use serenity::async_trait;
use serenity::builder::{CreateActionRow, CreateInteractionResponse};
use serenity::model::application::Interaction;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use serenity::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod commands;
pub mod components;
pub mod messages;
pub mod state;
pub mod views;

use crate::config::Settings;
use crate::torrent_client::TorrentClient;
pub use state::Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        commands::register_commands(&ctx, &ready).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let discord_id = match &interaction {
            Interaction::Command(cmd) => cmd.user.id.to_string(),
            Interaction::Component(comp) => comp.user.id.to_string(),
            _ => return,
        };

        let settings = self.settings.read().await.clone();
        let mut user = state::get_resolved_user(&settings, &discord_id);

        // Auto auth first administrator
        let has_admin = settings.users.iter().any(|u| {
            u.role == "administrator" && u.discord_id.as_deref().unwrap_or("") != "9876543210123"
        });
        if !has_admin {
            user.role = "administrator".to_string();
            user.discord_id = Some(discord_id.clone());
            {
                let mut settings_write = self.settings.write().await;
                settings_write.users.push(user.clone());
                settings_write.export_settings();
                let _ = crate::db::save_user_to_db(&user);
            }
            if let Interaction::Command(ref cmd) = interaction {
                let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                    serenity::builder::CreateInteractionResponseMessage::new().content("👑 You have been automatically authorized as the first **administrator**!").ephemeral(true)
                )).await;
            }
            return;
        }

        // Check authorization
        let is_request_access = match &interaction {
            Interaction::Component(comp) => comp.data.custom_id == "dc_request_access",
            _ => false,
        };
        let is_auth_interaction = match &interaction {
            Interaction::Component(comp) => comp.data.custom_id.starts_with("dc_auth:"),
            _ => false,
        };

        if settings
            .users
            .iter()
            .all(|u| u.discord_id.as_deref() != Some(&discord_id))
            && !is_request_access
            && !is_auth_interaction
        {
            let row = CreateActionRow::Buttons(vec![serenity::builder::CreateButton::new(
                "dc_request_access",
            )
            .label("Request Access")
            .style(serenity::model::application::ButtonStyle::Primary)]);
            let content = "❌ You are not authorized to use this bot.";
            match interaction {
                Interaction::Command(cmd) => {
                    let _ = cmd
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content(content)
                                    .components(vec![row])
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
                Interaction::Component(comp) => {
                    let _ = comp
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content(content)
                                    .components(vec![row])
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
                _ => {}
            }
            return;
        }

        match &interaction {
            Interaction::Command(cmd) => {
                commands::handle_command(self, &ctx, cmd, &user, &settings).await;
            }
            Interaction::Component(comp) => {
                components::handle_component(self, &ctx, comp, &user, &settings).await;
            }
            _ => {}
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        messages::handle_message(self, &ctx, msg).await;
    }
}

pub async fn start_discord_bot(
    settings: Arc<RwLock<Settings>>,
    manager: Arc<dyn TorrentClient>,
) -> Result<Client, String> {
    let token = {
        let s = settings.read().await;
        s.discord
            .token
            .clone()
            .ok_or_else(|| "Discord token not configured".to_string())?
    };

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler { settings, manager };

    let client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| e.to_string())?;

    Ok(client)
}
