.PHONY: help build up down logs clean test backup restore

help:
	@echo "Settlement Engine - Docker Commands"
	@echo ""
	@echo "Development:"
	@echo "  make build       - Build Docker images"
	@echo "  make up          - Start all services"
	@echo "  make down        - Stop all services"
	@echo "  make logs        - View logs"
	@echo "  make clean       - Remove all containers and volumes"
	@echo ""
	@echo "Production:"
	@echo "  make prod-up     - Start production stack"
	@echo "  make prod-down   - Stop production stack"
	@echo ""
	@echo "Database:"
	@echo "  make backup      - Create database backup"
	@echo "  make restore     - Restore database from backup"
	@echo "  make migrate     - Run database migrations"
	@echo ""
	@echo "Testing:"
	@echo "  make test        - Run all tests"
	@echo "  make bench       - Run benchmarks"

build:
	docker-compose build

up:
	docker-compose up -d
	@echo "Waiting for services to be healthy..."
	@sleep 10
	@docker-compose ps

down:
	docker-compose down

logs:
	docker-compose logs -f

clean:
	docker-compose down -v
	@echo "All containers and volumes removed"

prod-up:
	docker-compose -f docker-compose.prod.yml up -d
	@echo "Production stack started"

prod-down:
	docker-compose -f docker-compose.prod.yml down

backup:
	@echo "Creating database backup..."
	@docker-compose exec postgres pg_dump -U postgres settlement_engine | gzip > backups/manual_backup_$$(date +%Y%m%d_%H%M%S).sql.gz
	@echo "Backup created in backups/"

restore:
	@echo "Available backups:"
	@ls -lh backups/*.sql.gz
	@echo ""
	@read -p "Enter backup filename: " backup; \
	gunzip -c backups/$$backup | docker-compose exec -T postgres psql -U postgres settlement_engine

migrate:
	docker-compose exec app sqlx migrate run

test:
	cargo test --all-features

bench:
	cargo bench

shell-app:
	docker-compose exec app sh

shell-db:
	docker-compose exec postgres psql -U postgres settlement_engine

shell-redis:
	docker-compose exec redis redis-cli
