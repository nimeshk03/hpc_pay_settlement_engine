#!/bin/sh
# PostgreSQL backup script for Settlement Engine
# Runs daily via cron

set -e

BACKUP_DIR="/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/settlement_engine_${TIMESTAMP}.sql.gz"
RETENTION_DAYS=7

echo "Starting backup at $(date)"

# Create backup directory if it doesn't exist
mkdir -p "${BACKUP_DIR}"

# Perform backup
pg_dump -U "${POSTGRES_USER}" -h "${PGHOST}" "${POSTGRES_DB}" | gzip > "${BACKUP_FILE}"

if [ $? -eq 0 ]; then
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
