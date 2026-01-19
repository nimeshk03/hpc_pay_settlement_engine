use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::api::requests::{
    CreateAccountRequest, CreateTransactionRequest, ListBatchesQuery, ListLedgerEntriesQuery,
    ListTransactionsQuery, ProcessBatchRequest, ReverseTransactionRequest,
};
use crate::api::responses::{
    AccountResponse, ApiResponse, BalanceResponse, BatchResponse, ErrorResponse, HealthResponse,
    LedgerEntryResponse, PaginatedResponse, ServiceHealth, TransactionResponse,
    ValidationErrorDetail,
};
use crate::error::AppError;
use crate::models::{BatchStatus, TransactionStatus};
use crate::services::{
    AccountService, BalanceService, BatchService, LedgerService, LedgerTransactionRequest,
};

use super::routes::AppState;

/// Health check endpoint.
pub async fn health_check(State(state): State<AppState>) -> Json<ApiResponse<HealthResponse>> {
    let db_healthy = sqlx::query("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    let redis_healthy = state.redis_client.get_multiplexed_async_connection().await.is_ok();

    let kafka_healthy = state.kafka_connected();

    let response = HealthResponse {
        status: if db_healthy && redis_healthy { "healthy".to_string() } else { "degraded".to_string() },
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now(),
        services: ServiceHealth {
            database: db_healthy,
            redis: redis_healthy,
            kafka: kafka_healthy,
        },
    };

    Json(ApiResponse::success(response))
}

/// Readiness check endpoint.
pub async fn readiness_check(State(state): State<AppState>) -> StatusCode {
    let db_healthy = sqlx::query("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    if db_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

/// Liveness check endpoint.
pub async fn liveness_check() -> StatusCode {
    StatusCode::OK
}

// ============================================================================
// Account Handlers
// ============================================================================

/// Create a new account.
pub async fn create_account(
    State(state): State<AppState>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<ApiResponse<AccountResponse>>), (StatusCode, Json<ApiResponse<()>>)> {
    if let Err(errors) = request.validate() {
        let details: Vec<ValidationErrorDetail> = errors
            .iter()
            .map(|e| ValidationErrorDetail {
                field: e.field.clone(),
                message: e.message.clone(),
            })
            .collect();

        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(
                ErrorResponse::new("VALIDATION_ERROR", "Request validation failed")
                    .with_details(details),
            )),
        ));
    }

    let account_service = AccountService::new(state.pool.clone());

    let service_request = crate::services::account_service::CreateAccountRequest {
        external_id: request.external_id,
        name: request.name,
        account_type: request.account_type,
        currency: request.currency,
        initial_balance: request.initial_balance,
        metadata: request.metadata,
    };

    match account_service.create_account(service_request).await {
        Ok(account) => Ok((
            StatusCode::CREATED,
            Json(ApiResponse::success(AccountResponse::from(account))),
        )),
        Err(AppError::Validation(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(ErrorResponse::new("VALIDATION_ERROR", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to create account: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Get account by ID.
pub async fn get_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<AccountResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let account_service = AccountService::new(state.pool.clone());

    match account_service.find_by_id(id).await {
        Ok(account) => Ok(Json(ApiResponse::success(AccountResponse::from(account)))),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to get account: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Get account balance.
pub async fn get_account_balance(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<BalanceResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let balance_service = BalanceService::new(state.pool.clone());
    let account_service = AccountService::new(state.pool.clone());

    let account = match account_service.find_by_id(id).await {
        Ok(acc) => acc,
        Err(AppError::NotFound(msg)) => return Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to get account for balance: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new("INTERNAL_ERROR", "An internal error occurred"))),
            ));
        }
    };

    match balance_service.get_balance(id, &account.currency).await {
        Ok(balance) => Ok(Json(ApiResponse::success(BalanceResponse::from(balance)))),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to get balance: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Get account ledger entries.
pub async fn get_account_ledger(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<ListLedgerEntriesQuery>,
) -> Result<Json<ApiResponse<PaginatedResponse<LedgerEntryResponse>>>, (StatusCode, Json<ApiResponse<()>>)>
{
    let ledger_service = LedgerService::new(state.pool.clone());
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let total = match ledger_service.count_account_ledger_entries(id).await {
        Ok(count) => count,
        Err(e) => {
            tracing::error!("Failed to count ledger entries: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ));
        }
    };

    match ledger_service.get_account_ledger_entries(id, limit, offset).await {
        Ok(entries) => {
            let response_entries: Vec<LedgerEntryResponse> =
                entries.iter().cloned().map(LedgerEntryResponse::from).collect();
            Ok(Json(ApiResponse::success(PaginatedResponse::new(
                response_entries,
                total,
                limit,
                offset,
            ))))
        }
        Err(e) => {
            tracing::error!("Failed to get ledger entries: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

// ============================================================================
// Transaction Handlers
// ============================================================================

/// Create a new transaction.
pub async fn create_transaction(
    State(state): State<AppState>,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<TransactionResponse>>), (StatusCode, Json<ApiResponse<()>>)> {
    if let Err(errors) = request.validate() {
        let details: Vec<ValidationErrorDetail> = errors
            .iter()
            .map(|e| ValidationErrorDetail {
                field: e.field.clone(),
                message: e.message.clone(),
            })
            .collect();

        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(
                ErrorResponse::new("VALIDATION_ERROR", "Request validation failed")
                    .with_details(details),
            )),
        ));
    }

    let ledger_service = LedgerService::new(state.pool.clone());

    let ledger_request = LedgerTransactionRequest {
        external_id: request.external_id,
        transaction_type: request.transaction_type,
        source_account_id: request.source_account_id,
        destination_account_id: request.destination_account_id,
        amount: request.amount,
        currency: request.currency,
        fee_amount: request.fee_amount.unwrap_or(Decimal::ZERO),
        idempotency_key: request.idempotency_key,
        effective_date: None,
        metadata: request.metadata,
        original_transaction_id: None,
    };

    match ledger_service.process_transaction(ledger_request).await {
        Ok(result) => Ok((
            StatusCode::CREATED,
            Json(ApiResponse::success(TransactionResponse::from(result.transaction))),
        )),
        Err(AppError::Validation(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(ErrorResponse::new("VALIDATION_ERROR", msg))),
        )),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to create transaction: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Get transaction by ID.
pub async fn get_transaction(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<TransactionResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let ledger_service = LedgerService::new(state.pool.clone());

    match ledger_service.get_transaction(id).await {
        Ok(tx) => Ok(Json(ApiResponse::success(TransactionResponse::from(tx)))),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to get transaction: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// List transactions with filters.
pub async fn list_transactions(
    State(state): State<AppState>,
    Query(query): Query<ListTransactionsQuery>,
) -> Result<Json<ApiResponse<PaginatedResponse<TransactionResponse>>>, (StatusCode, Json<ApiResponse<()>>)>
{
    let ledger_service = LedgerService::new(state.pool.clone());
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let status = match query.status.as_ref() {
        Some(s) => match s.to_uppercase().as_str() {
            "PENDING" => Some(TransactionStatus::Pending),
            "SETTLED" => Some(TransactionStatus::Settled),
            "FAILED" => Some(TransactionStatus::Failed),
            "REVERSED" => Some(TransactionStatus::Reversed),
            _ => return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "VALIDATION_ERROR",
                    format!("Invalid status '{}'. Valid values: PENDING, SETTLED, FAILED, REVERSED", s),
                ))),
            )),
        },
        None => None,
    };

    let total = match ledger_service.count_transactions(query.account_id, status, query.currency.as_deref()).await {
        Ok(count) => count,
        Err(e) => {
            tracing::error!("Failed to count transactions: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ));
        }
    };

    match ledger_service
        .list_transactions(query.account_id, status, query.currency.as_deref(), limit, offset)
        .await
    {
        Ok(transactions) => {
            let response_txs: Vec<TransactionResponse> =
                transactions.iter().cloned().map(TransactionResponse::from).collect();
            Ok(Json(ApiResponse::success(PaginatedResponse::new(
                response_txs,
                total,
                limit,
                offset,
            ))))
        }
        Err(e) => {
            tracing::error!("Failed to list transactions: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Reverse a transaction.
pub async fn reverse_transaction(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<ReverseTransactionRequest>,
) -> Result<Json<ApiResponse<TransactionResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    if let Err(errors) = request.validate() {
        let details: Vec<ValidationErrorDetail> = errors
            .iter()
            .map(|e| ValidationErrorDetail {
                field: e.field.clone(),
                message: e.message.clone(),
            })
            .collect();

        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(
                ErrorResponse::new("VALIDATION_ERROR", "Request validation failed")
                    .with_details(details),
            )),
        ));
    }

    let ledger_service = LedgerService::new(state.pool.clone());

    match ledger_service
        .reverse_transaction(id, &request.reason, &request.idempotency_key)
        .await
    {
        Ok(result) => Ok(Json(ApiResponse::success(TransactionResponse::from(
            result.transaction,
        )))),
        Err(AppError::Validation(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(ErrorResponse::new("VALIDATION_ERROR", msg))),
        )),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to reverse transaction: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

// ============================================================================
// Batch Handlers
// ============================================================================

/// List batches with filters.
pub async fn list_batches(
    State(state): State<AppState>,
    Query(query): Query<ListBatchesQuery>,
) -> Result<Json<ApiResponse<PaginatedResponse<BatchResponse>>>, (StatusCode, Json<ApiResponse<()>>)> {
    let batch_service = BatchService::new(state.pool.clone());
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let status = query.status.as_ref().and_then(|s| match s.to_uppercase().as_str() {
        "PENDING" => Some(BatchStatus::Pending),
        "PROCESSING" => Some(BatchStatus::Processing),
        "COMPLETED" => Some(BatchStatus::Completed),
        "FAILED" => Some(BatchStatus::Failed),
        _ => None,
    });

    match batch_service
        .list_batches(status, query.currency.as_deref(), limit, offset)
        .await
    {
        Ok(batches) => {
            let response_batches: Vec<BatchResponse> =
                batches.iter().cloned().map(BatchResponse::from).collect();
            let total = response_batches.len() as i64;
            Ok(Json(ApiResponse::success(PaginatedResponse::new(
                response_batches,
                total,
                limit,
                offset,
            ))))
        }
        Err(e) => {
            tracing::error!("Failed to list batches: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Get batch by ID.
pub async fn get_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<BatchResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let batch_service = BatchService::new(state.pool.clone());

    match batch_service.get_batch(id).await {
        Ok(batch) => Ok(Json(ApiResponse::success(BatchResponse::from(batch)))),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to get batch: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Process a batch.
pub async fn process_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(_request): Json<ProcessBatchRequest>,
) -> Result<Json<ApiResponse<BatchResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    let batch_service = BatchService::new(state.pool.clone());

    match batch_service.process_batch(id).await {
        Ok(_result) => {
            match batch_service.get_batch(id).await {
                Ok(batch) => Ok(Json(ApiResponse::success(BatchResponse::from(batch)))),
                Err(e) => {
                    tracing::error!("Failed to get batch after processing: {}", e);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::<()>::error(ErrorResponse::new(
                            "INTERNAL_ERROR",
                            "An internal error occurred",
                        ))),
                    ))
                }
            }
        }
        Err(AppError::Validation(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(ErrorResponse::new("VALIDATION_ERROR", msg))),
        )),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to process batch: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}

/// Get batch netting positions.
pub async fn get_batch_positions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<crate::models::NettingPosition>>>, (StatusCode, Json<ApiResponse<()>>)> {
    let batch_service = BatchService::new(state.pool.clone());

    match batch_service.get_batch_positions(id).await {
        Ok(positions) => Ok(Json(ApiResponse::success(positions))),
        Err(AppError::NotFound(msg)) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error(ErrorResponse::new("NOT_FOUND", msg))),
        )),
        Err(e) => {
            tracing::error!("Failed to get batch positions: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(ErrorResponse::new(
                    "INTERNAL_ERROR",
                    "An internal error occurred",
                ))),
            ))
        }
    }
}
