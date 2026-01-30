#!/bin/sh
# PostgreSQL backup script for Settlement Engine
# Runs daily via cron

set -e

# Validate required environment variables
if [ -z "${POSTGRES_USER}" ]; then
    echo "ERROR: POSTGRES_USER environment variable is not set" >&2
    exit 1
fi

if [ -z "${PGHOST}" ]; then
    echo "ERROR: PGHOST environment variable is not set" >&2
    exit 1
fi

if [ -z "${POSTGRES_DB}" ]; then
    echo "ERROR: POSTGRES_DB environment variable is not set" >&2
    exit 1
fi

BACKUP_DIR="/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/settlement_engine_${TIMESTAMP}.sql.gz"
RETENTION_DAYS=7

echo "Starting backup at $(date)"

# Create backup directory if it doesn't exist
mkdir -p "${BACKUP_DIR}"

# Perform backup with proper error handling
TEMP_FILE="${BACKUP_DIR}/temp_backup_${TIMESTAMP}.sql"

pg_dump -U "${POSTGRES_USER}" -h "${PGHOST}" "${POSTGRES_DB}" > "${TEMP_FILE}"
PG_DUMP_EXIT=$?

if [ ${PG_DUMP_EXIT} -eq 0 ]; then
    gzip < "${TEMP_FILE}" > "${BACKUP_FILE}"
    GZIP_EXIT=$?
    rm -f "${TEMP_FILE}"
    
    if [ ${GZIP_EXIT} -ne 0 ]; then
        echo "Gzip compression failed!"
        exit 1
    fi
    echo "Backup completed successfully: ${BACKUP_FILE}"
    
    # Calculate backup size
    BACKUP_SIZE=$(du -h "${BACKUP_FILE}" | cut -f1)
    echo "Backup size: ${BACKUP_SIZE}"
    
    # Remove old backups
    find "${BACKUP_DIR}" -name "settlement_engine_*.sql.gz" -type f -mtime +${RETENTION_DAYS} -delete
    echo "Removed backups older than ${RETENTION_DAYS} days"
    
    # List remaining backups
    echo "Current backups:"
    ls -lh "${BACKUP_DIR}"/settlement_engine_*.sql.gz 2>/dev/null || echo "No backups found"
else
    echo "Backup failed!"
    exit 1
fi

echo "Backup process completed at $(date)"
