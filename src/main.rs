//! OpenSASE Scheduling - Self-hosted Appointment Scheduling

use anyhow::Result;
use axum::{extract::{Path, Query, State}, http::StatusCode, routing::{get, post, put, delete}, Json, Router};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Service {
    pub id: Uuid, pub name: String, pub description: Option<String>,
    pub duration_minutes: i32, pub price: Option<i64>, pub currency: String,
    pub status: String, pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Appointment {
    pub id: Uuid, pub service_id: Uuid, pub customer_name: String, pub customer_email: String,
    pub customer_phone: Option<String>, pub scheduled_date: NaiveDate, pub scheduled_time: NaiveTime,
    pub duration_minutes: i32, pub status: String, pub notes: Option<String>,
    pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Availability {
    pub id: Uuid, pub day_of_week: i32, pub start_time: NaiveTime, pub end_time: NaiveTime,
    pub is_available: bool, pub created_at: DateTime<Utc>,
}

#[derive(Clone)] pub struct AppState { pub db: sqlx::PgPool }

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).with(tracing_subscriber::fmt::layer()).init();
    let db = PgPoolOptions::new().max_connections(10).connect(&std::env::var("DATABASE_URL")?).await?;
    sqlx::migrate!("./migrations").run(&db).await?;
    let state = AppState { db };

    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({"status": "healthy", "service": "opensase-scheduling"})) }))
        .route("/api/v1/services", get(list_services).post(create_service))
        .route("/api/v1/services/:id", get(get_service).put(update_service).delete(delete_service))
        .route("/api/v1/appointments", get(list_appointments).post(create_appointment))
        .route("/api/v1/appointments/:id", get(get_appointment).put(update_appointment))
        .route("/api/v1/appointments/:id/cancel", post(cancel_appointment))
        .route("/api/v1/availability", get(get_availability).post(set_availability))
        .route("/api/v1/slots", get(get_available_slots))
        .layer(TraceLayer::new_for_http()).layer(CorsLayer::permissive()).with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8088".to_string());
    tracing::info!("🚀 OpenSASE Scheduling listening on 0.0.0.0:{}", port);
    axum::serve(tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?, app).await?;
    Ok(())
}

#[derive(Debug, Deserialize)] pub struct ListParams { pub page: Option<u32>, pub per_page: Option<u32>, pub date: Option<NaiveDate> }
#[derive(Debug, Serialize)] pub struct PaginatedResponse<T> { pub data: Vec<T>, pub total: i64, pub page: u32 }

// Service endpoints
async fn list_services(State(s): State<AppState>) -> Result<Json<Vec<Service>>, (StatusCode, String)> {
    let services = sqlx::query_as::<_, Service>("SELECT * FROM services WHERE status = 'active' ORDER BY name").fetch_all(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(services))
}

async fn get_service(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Service>, (StatusCode, String)> {
    sqlx::query_as::<_, Service>("SELECT * FROM services WHERE id = $1").bind(id).fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.map(Json).ok_or((StatusCode::NOT_FOUND, "Not found".to_string()))
}

#[derive(Debug, Deserialize)] pub struct CreateServiceRequest { pub name: String, pub description: Option<String>, pub duration_minutes: i32, pub price: Option<i64> }

async fn create_service(State(s): State<AppState>, Json(r): Json<CreateServiceRequest>) -> Result<(StatusCode, Json<Service>), (StatusCode, String)> {
    let svc = sqlx::query_as::<_, Service>("INSERT INTO services (id, name, description, duration_minutes, price, currency, status, created_at) VALUES ($1, $2, $3, $4, $5, 'NGN', 'active', NOW()) RETURNING *")
        .bind(Uuid::now_v7()).bind(&r.name).bind(&r.description).bind(r.duration_minutes).bind(r.price)
        .fetch_one(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(svc)))
}

async fn update_service(State(s): State<AppState>, Path(id): Path<Uuid>, Json(r): Json<CreateServiceRequest>) -> Result<Json<Service>, (StatusCode, String)> {
    let svc = sqlx::query_as::<_, Service>("UPDATE services SET name = $2, description = $3, duration_minutes = $4, price = $5 WHERE id = $1 RETURNING *")
        .bind(id).bind(&r.name).bind(&r.description).bind(r.duration_minutes).bind(r.price)
        .fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.ok_or((StatusCode::NOT_FOUND, "Not found".to_string()))?;
    Ok(Json(svc))
}

async fn delete_service(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, (StatusCode, String)> {
    sqlx::query("UPDATE services SET status = 'deleted' WHERE id = $1").bind(id).execute(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// Appointment endpoints
async fn list_appointments(State(s): State<AppState>, Query(p): Query<ListParams>) -> Result<Json<Vec<Appointment>>, (StatusCode, String)> {
    let appts = if let Some(date) = p.date {
        sqlx::query_as::<_, Appointment>("SELECT * FROM appointments WHERE scheduled_date = $1 ORDER BY scheduled_time").bind(date).fetch_all(&s.db).await
    } else {
        sqlx::query_as::<_, Appointment>("SELECT * FROM appointments ORDER BY scheduled_date DESC, scheduled_time").fetch_all(&s.db).await
    }.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(appts))
}

async fn get_appointment(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Appointment>, (StatusCode, String)> {
    sqlx::query_as::<_, Appointment>("SELECT * FROM appointments WHERE id = $1").bind(id).fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.map(Json).ok_or((StatusCode::NOT_FOUND, "Not found".to_string()))
}

#[derive(Debug, Deserialize)] pub struct CreateAppointmentRequest { pub service_id: Uuid, pub customer_name: String, pub customer_email: String, pub customer_phone: Option<String>, pub scheduled_date: NaiveDate, pub scheduled_time: NaiveTime, pub notes: Option<String> }

async fn create_appointment(State(s): State<AppState>, Json(r): Json<CreateAppointmentRequest>) -> Result<(StatusCode, Json<Appointment>), (StatusCode, String)> {
    let svc = sqlx::query_as::<_, Service>("SELECT * FROM services WHERE id = $1").bind(r.service_id).fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.ok_or((StatusCode::BAD_REQUEST, "Service not found".to_string()))?;
    let appt = sqlx::query_as::<_, Appointment>("INSERT INTO appointments (id, service_id, customer_name, customer_email, customer_phone, scheduled_date, scheduled_time, duration_minutes, status, notes, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'confirmed', $9, NOW(), NOW()) RETURNING *")
        .bind(Uuid::now_v7()).bind(r.service_id).bind(&r.customer_name).bind(&r.customer_email).bind(&r.customer_phone).bind(r.scheduled_date).bind(r.scheduled_time).bind(svc.duration_minutes).bind(&r.notes)
        .fetch_one(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(appt)))
}

async fn update_appointment(State(s): State<AppState>, Path(id): Path<Uuid>, Json(r): Json<CreateAppointmentRequest>) -> Result<Json<Appointment>, (StatusCode, String)> {
    let appt = sqlx::query_as::<_, Appointment>("UPDATE appointments SET scheduled_date = $2, scheduled_time = $3, notes = $4, updated_at = NOW() WHERE id = $1 RETURNING *")
        .bind(id).bind(r.scheduled_date).bind(r.scheduled_time).bind(&r.notes)
        .fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.ok_or((StatusCode::NOT_FOUND, "Not found".to_string()))?;
    Ok(Json(appt))
}

async fn cancel_appointment(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Appointment>, (StatusCode, String)> {
    let appt = sqlx::query_as::<_, Appointment>("UPDATE appointments SET status = 'cancelled', updated_at = NOW() WHERE id = $1 RETURNING *")
        .bind(id).fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.ok_or((StatusCode::NOT_FOUND, "Not found".to_string()))?;
    Ok(Json(appt))
}

// Availability endpoints
async fn get_availability(State(s): State<AppState>) -> Result<Json<Vec<Availability>>, (StatusCode, String)> {
    let avail = sqlx::query_as::<_, Availability>("SELECT * FROM availability ORDER BY day_of_week, start_time").fetch_all(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(avail))
}

#[derive(Debug, Deserialize)] pub struct SetAvailabilityRequest { pub day_of_week: i32, pub start_time: NaiveTime, pub end_time: NaiveTime, pub is_available: bool }

async fn set_availability(State(s): State<AppState>, Json(r): Json<SetAvailabilityRequest>) -> Result<(StatusCode, Json<Availability>), (StatusCode, String)> {
    let avail = sqlx::query_as::<_, Availability>("INSERT INTO availability (id, day_of_week, start_time, end_time, is_available, created_at) VALUES ($1, $2, $3, $4, $5, NOW()) ON CONFLICT (day_of_week) DO UPDATE SET start_time = $3, end_time = $4, is_available = $5 RETURNING *")
        .bind(Uuid::now_v7()).bind(r.day_of_week).bind(r.start_time).bind(r.end_time).bind(r.is_available)
        .fetch_one(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::OK, Json(avail)))
}

#[derive(Debug, Deserialize)] pub struct SlotsQuery { pub date: NaiveDate, pub service_id: Uuid }
#[derive(Debug, Serialize)] pub struct TimeSlot { pub time: NaiveTime, pub available: bool }

async fn get_available_slots(State(s): State<AppState>, Query(q): Query<SlotsQuery>) -> Result<Json<Vec<TimeSlot>>, (StatusCode, String)> {
    let svc = sqlx::query_as::<_, Service>("SELECT * FROM services WHERE id = $1").bind(q.service_id).fetch_optional(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.ok_or((StatusCode::BAD_REQUEST, "Service not found".to_string()))?;
    
    // Get booked appointments for the date
    let booked: Vec<Appointment> = sqlx::query_as("SELECT * FROM appointments WHERE scheduled_date = $1 AND status != 'cancelled'")
        .bind(q.date).fetch_all(&s.db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    // Generate slots (9 AM to 5 PM, every 30 minutes)
    let mut slots = Vec::new();
    let mut time = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let end = NaiveTime::from_hms_opt(17, 0, 0).unwrap();
    
    while time < end {
        let is_booked = booked.iter().any(|a| a.scheduled_time == time);
        slots.push(TimeSlot { time, available: !is_booked });
        time = time + chrono::Duration::minutes(svc.duration_minutes as i64);
    }
    
    Ok(Json(slots))
}
