use serenity::prelude::*;
use serenity::model::application::{ComponentInteraction, ButtonStyle, ComponentInteractionDataKind};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse, CreateButton, CreateActionRow};
use serenity::model::id::UserId;

use crate::config::{Settings, UserSettings};
use super::state::Handler;
use super::views;

pub async fn handle_component(
    handler: &Handler,
    ctx: &Context,
    comp: &ComponentInteraction,
    user: &UserSettings,
    settings: &Settings,
) {
    let custom_id = comp.data.custom_id.as_str();
    let interaction = serenity::model::application::Interaction::Component(comp.clone());
    if custom_id == "select_torrent" {
        if let ComponentInteractionDataKind::StringSelect { values } = &comp.data.kind {
            if let Some(hash) = values.first() {
                let _ = views::show_torrent_details(handler, ctx, &interaction, user, hash, settings).await;
            }
        }
    } else if custom_id == "dc_back_list" {
        if let Err(e) = views::show_torrent_list(handler, ctx, &interaction, user, None).await {
            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e)).ephemeral(true)
            )).await;
        }
    } else if custom_id == "dc_request_access" {
        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new().content("⏳ Access request sent to administrators. Please wait...").components(vec![])
        )).await;

        for u in &settings.users {
            if u.role == "administrator" && u.discord_id.is_some() && u.discord_id.as_deref() != Some("9876543210123") {
                if let Some(ref admin_dc_id) = u.discord_id {
                    if let Ok(admin_user_id) = admin_dc_id.parse::<u64>() {
                        if let Ok(admin_user) = UserId::new(admin_user_id).to_user(&ctx.http).await {
                            let approve_admin = CreateButton::new(format!("dc_auth:approve:administrator:{}:{}", comp.user.id, comp.user.name))
                                .label("Approve Admin").style(ButtonStyle::Success);
                            let approve_manager = CreateButton::new(format!("dc_auth:approve:manager:{}:{}", comp.user.id, comp.user.name))
                                .label("Approve Manager").style(ButtonStyle::Primary);
                            let approve_reader = CreateButton::new(format!("dc_auth:approve:reader:{}:{}", comp.user.id, comp.user.name))
                                .label("Approve Reader").style(ButtonStyle::Secondary);
                            let deny_btn = CreateButton::new(format!("dc_auth:deny:{}:{}", comp.user.id, comp.user.name))
                                .label("Deny").style(ButtonStyle::Danger);
                            
                            let row = CreateActionRow::Buttons(vec![approve_admin, approve_manager, approve_reader, deny_btn]);
                            let _ = admin_user.dm(&ctx.http, serenity::builder::CreateMessage::new()
                                .content(format!("🔔 User **{}** (ID: `{}`) is requesting access to the bot.", comp.user.name, comp.user.id))
                                .components(vec![row])
                            ).await;
                        }
                    }
                }
            }
        }
    } else if custom_id.starts_with("dc_auth:") {
        let parts: Vec<&str> = custom_id.split(':').collect();
        let action = parts[1];
        if action == "approve" {
            let role = parts[2];
            let target_user_id_str = parts[3];
            let username = parts.get(4).cloned().unwrap_or("User");
            
            let mut target_user = UserSettings {
                user_id: 0,
                discord_id: Some(target_user_id_str.to_string()),
                role: role.to_string(),
                locale: Some("en".to_string()),
                notify: true,
                notification_filter: vec![],
            };

            {
                let mut settings_write = handler.settings.write().await;
                if let Some(existing) = settings_write.users.iter_mut().find(|u| u.discord_id.as_deref() == Some(target_user_id_str)) {
                    existing.role = role.to_string();
                    target_user = existing.clone();
                } else {
                    settings_write.users.push(target_user.clone());
                }
                settings_write.export_settings();
            }

            let _ = crate::db::save_user_to_db(&target_user);

            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new().content(format!("✅ Authorized **{}** (ID: `{}`) as **{}**.", username, target_user_id_str, role)).components(vec![])
            )).await;

            if let Ok(uid) = target_user_id_str.parse::<u64>() {
                if let Ok(tu) = UserId::new(uid).to_user(&ctx.http).await {
                    let _ = tu.dm(&ctx.http, serenity::builder::CreateMessage::new()
                        .content(format!("🎉 Your access request has been approved! You now have **{}** role.", role))
                    ).await;
                }
            }
        } else if action == "deny" {
            let target_user_id_str = parts[2];
            let username = parts.get(3).cloned().unwrap_or("User");

            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new().content(format!("❌ Access request for **{}** (ID: `{}`) was denied.", username, target_user_id_str)).components(vec![])
            )).await;

            if let Ok(uid) = target_user_id_str.parse::<u64>() {
                if let Ok(tu) = UserId::new(uid).to_user(&ctx.http).await {
                    let _ = tu.dm(&ctx.http, serenity::builder::CreateMessage::new()
                        .content("❌ Your access request was denied by an administrator.")
                    ).await;
                }
            }
        }
    } else if custom_id.starts_with("dc_status:") {
        let status = custom_id.split(':').nth(1);
        if let Err(e) = views::show_torrent_list(handler, ctx, &interaction, user, status).await {
            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e)).ephemeral(true)
            )).await;
        }
    } else if custom_id.starts_with("dc_get_link:") {
        let hash = custom_id.split(':').nth(1).unwrap_or("");
        let _ = comp.defer_ephemeral(&ctx.http).await;

        match handler.manager.get_torrent(hash, None).await {
            Ok(Some(torrent)) => {
                match crate::s3::get_download_link(settings, &torrent).await {
                    Ok(Some(url)) => {
                        let row = CreateActionRow::Buttons(vec![
                            CreateButton::new_link(url).label("Download")
                        ]);
                        let content = if settings.local_server.enabled {
                            format!("🔗 Here is your download link for **{}**:", torrent.name)
                        } else {
                            format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", torrent.name)
                        };
                        let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new()
                            .content(content)
                            .components(vec![row])
                        ).await;
                    }
                    Ok(None) => {
                        let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Download links are either disabled or not configured.")).await;
                    }
                    Err(e) => {
                        let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("Error: {}", e))).await;
                    }
                }
            }
            _ => {
                let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new().content("Torrent not found.")).await;
            }
        }
    } else if custom_id.starts_with("dc_pause:") {
        if user.role == "reader" {
            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("Unauthorized.").ephemeral(true)
            )).await;
            return;
        }
        let hash = custom_id.split(':').nth(1).unwrap_or("");
        let _ = handler.manager.pause(hash).await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let _ = views::show_torrent_details(handler, ctx, &interaction, user, hash, settings).await;
    } else if custom_id.starts_with("dc_resume:") {
        if user.role == "reader" {
            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("Unauthorized.").ephemeral(true)
            )).await;
            return;
        }
        let hash = custom_id.split(':').nth(1).unwrap_or("");
        let _ = handler.manager.resume(hash).await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let _ = views::show_torrent_details(handler, ctx, &interaction, user, hash, settings).await;
    } else if custom_id.starts_with("dc_delete:") {
        if user.role != "administrator" {
            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("Unauthorized. Only administrators can delete torrents.").ephemeral(true)
            )).await;
            return;
        }
        let hash = custom_id.split(':').nth(1).unwrap_or("");
        let confirm_no_data = CreateButton::new(format!("dc_cldel:{}:false", hash))
            .label("Delete (Keep Files)").style(ButtonStyle::Danger);
        let confirm_data = CreateButton::new(format!("dc_cldel:{}:true", hash))
            .label("Delete EVERYTHING").style(ButtonStyle::Danger);
        let cancel_btn = CreateButton::new(format!("dc_detail:{}", hash))
            .label("Cancel").style(ButtonStyle::Secondary);
        
        let row = CreateActionRow::Buttons(vec![confirm_no_data, confirm_data, cancel_btn]);
        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new().content("⚠️ **Are you sure you want to delete this torrent?**").embeds(vec![]).components(vec![row])
        )).await;
    } else if custom_id.starts_with("dc_cldel:") {
        if user.role != "administrator" {
            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("Unauthorized.").ephemeral(true)
            )).await;
            return;
        }
        let parts: Vec<&str> = custom_id.split(':').collect();
        let hash = parts[1];
        let delete_files = parts[2] == "true";

        let res = if delete_files {
            handler.manager.delete_one_data(hash).await
        } else {
            handler.manager.delete_one_no_data(hash).await
        };

        match res {
            Ok(_) => {
                let msg = format!("🗑️ Torrent deleted successfully{}.", if delete_files { " (including files)" } else { "" });
                let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new().content(msg).components(vec![])
                )).await;
            }
            Err(e) => {
                let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().content(format!("Failed to delete: {}", e)).ephemeral(true)
                )).await;
            }
        }
    } else if custom_id.starts_with("dc_detail:") {
        let hash = custom_id.split(':').nth(1).unwrap_or("");
        let _ = views::show_torrent_details(handler, ctx, &interaction, user, hash, settings).await;
    }
}
