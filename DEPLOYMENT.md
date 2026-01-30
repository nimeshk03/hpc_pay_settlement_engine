# Deployment Guide

## Prerequisites

- Docker 20.10+
- Docker Compose 2.0+
- 4GB RAM minimum
- 20GB disk space

## Development Deployment

### Quick Start

```bash
# Start all services
make up

# View logs
make logs

# Stop services
make down
```

### Manual Steps

1. **Start services**:
   ```bash
   docker-compose up -d
   ```

2. **Verify health**:
   ```bash
   docker-compose ps
   curl http://localhost:3000/health
   ```

3. **Run migrations** (automatic on startup):
   ```bash
   docker-compose exec app sqlx migrate run
   ```

## Production Deployment

### Pre-deployment Checklist

- [ ] Update secrets in `secrets/postgres_password.txt`
- [ ] Set environment variables in `.env`
- [ ] Configure resource limits in `docker-compose.prod.yml`
- [ ] Set up external monitoring (Prometheus/Grafana)
- [ ] Configure log aggregation (ELK/Loki)
- [ ] Set up backup retention policy
- [ ] Configure SSL/TLS certificates
- [ ] Review security settings

### Production Setup

1. **Create secrets**:
   ```bash
   # Retrieve password from your password manager or vault
   # Example using 1Password CLI:
   # op read "op://vault/postgres/password" > secrets/postgres_password.txt
   
   # Or read interactively (password won't appear in shell history):
   read -s -p "Enter PostgreSQL password: " PGPASS && echo "$PGPASS" > secrets/postgres_password.txt && unset PGPASS
   
   chmod 600 secrets/postgres_password.txt
   ```

2. **Configure environment**:
   ```bash
   cp .env.example .env
   # Edit .env with production values
   ```

3. **Start production stack**:
   ```bash
   docker-compose -f docker-compose.prod.yml up -d
   ```

4. **Verify deployment**:
   ```bash
   docker-compose -f docker-compose.prod.yml ps
   curl http://localhost:3000/health/detailed
   ```

## Kubernetes Deployment

### Prerequisites

- Kubernetes 1.24+
- kubectl configured
- Persistent volume provisioner

### Deploy to Kubernetes

1. **Create namespace**:
   ```bash
   kubectl apply -f k8s/deployment.yaml
   ```

2. **Update secrets**:
   ```bash
   kubectl create secret generic settlement-secrets \
     --from-literal=POSTGRES_PASSWORD=your_password \
     --from-literal=REDIS_PASSWORD=your_password \
     -n settlement-engine
   ```

3. **Verify deployment**:
   ```bash
   kubectl get pods -n settlement-engine
   kubectl get svc -n settlement-engine
   ```

## Database Backup and Restore

### Manual Backup

```bash
make backup
```

Or manually:
```bash
docker-compose exec postgres pg_dump -U postgres settlement_engine | \
  gzip > backups/backup_$(date +%Y%m%d_%H%M%S).sql.gz
```

### Restore from Backup

```bash
make restore
```

Or manually:
```bash
gunzip -c backups/backup_file.sql.gz | \
  docker-compose exec -T postgres psql -U postgres settlement_engine
```

### Automated Backups

**Docker Compose**: The production stack includes a `backup` service that runs `/backup.sh` via cron at 2 AM daily. The service mounts `./backups` and requires `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB` environment variables.

**Kubernetes**: Deploy a CronJob resource that runs the backup script. Set `BACKUP_SCHEDULE` (default: `0 2 * * *` for 2 AM daily) and configure the same environment variables.

- Retention: 7 days (configurable via `RETENTION_DAYS`)
- Location: `./backups/` (local volume)
- Format: `settlement_engine_YYYYMMDD_HHMMSS.sql.gz`
- **IMPORTANT**: Configure remote/offsite backup transfer (see Backup Strategy below)

## Monitoring

### Health Endpoints

- `/health` - Basic health check
- `/health/detailed` - Detailed dependency status
- `/ready` - Readiness probe
- `/live` - Liveness probe
- `/metrics` - Prometheus metrics

### Logs

View application logs:
```bash
docker-compose logs -f app
```

View all logs:
```bash
make logs
```

## Troubleshooting

### Services Not Starting

1. Check logs:
   ```bash
   docker-compose logs
   ```

2. Verify resource availability:
   ```bash
   docker stats
   ```

3. Check health status:
   ```bash
   docker-compose ps
   ```

### Database Connection Issues

1. Verify PostgreSQL is running:
   ```bash
   docker-compose exec postgres pg_isready -U postgres
   ```

2. Check connection string in environment variables

3. Verify network connectivity:
   ```bash
   docker-compose exec app ping postgres
   ```

### Performance Issues

1. Check resource usage:
   ```bash
   docker stats
   ```

2. Review metrics endpoint:
   ```bash
   curl http://localhost:3000/metrics
   ```

3. Analyze slow queries in PostgreSQL logs

## Scaling

### Horizontal Scaling (Docker Compose)

```bash
docker-compose up -d --scale app=3
```

### Kubernetes Auto-scaling

HPA is configured to scale between 2-10 replicas based on:
- CPU utilization: 70%
- Memory utilization: 80%

## Security Considerations

1. **Secrets Management**:
   - Never commit secrets to version control
   - Use Docker secrets or Kubernetes secrets
   - Rotate credentials regularly

2. **Network Security**:
   - Use internal networks for service communication
   - Expose only necessary ports
   - Configure firewall rules

3. **Container Security**:
   - Run as non-root user (already configured)
   - Keep base images updated
   - Scan images for vulnerabilities

## Disaster Recovery

### Backup Strategy

**MANDATORY for Production:**

- **Frequency**: Daily automated backups
- **Retention**: 7 days local, 90 days remote with lifecycle policies matching RTO/RPO
- **Storage**: 
  - **Primary**: Encrypted S3-compatible object storage with versioning and object lock enabled
  - **Secondary**: Cross-region replication to geographically separate location
  - **Recommended**: AWS S3, Google Cloud Storage, Azure Blob Storage, or MinIO with encryption
- **Encryption**: 
  - AES-256 encryption at rest (server-side encryption)
  - TLS 1.3 for encryption in transit
  - Encrypted backup files before upload
- **Transfer**: Automated sync from local `./backups/` to remote storage after each backup
- **Immutability**: Enable object lock or WORM (Write Once Read Many) on remote storage
- **Testing**: 
  - Monthly restore tests to verify RTO (< 15 minutes)
  - Quarterly disaster recovery drills
  - Automated integrity checks (checksums, test restores)
- **Monitoring**: Alert on backup failures, missing backups, or integrity check failures

### Recovery Procedure

1. Stop affected services
2. Restore from latest backup
3. Verify data integrity
4. Restart services
5. Monitor for issues

## Performance Targets

- **Throughput**: 50,000 TPS
- **Latency**: 
  - Balance query: < 1ms P99 (with cache)
  - Transaction processing: < 10ms P99
- **Availability**: 99.9% uptime
- **Recovery Time Objective (RTO)**: < 15 minutes
- **Recovery Point Objective (RPO)**: < 5 minutes
