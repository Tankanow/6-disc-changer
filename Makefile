
LOCAL_CONTAINER_NAME=6DC

.PHONY: cargo-run
cargo-run:
	cargo run

.PHONY: docker-run
docker-run: docker-stop
	docker build -t six-disc-changer .
	docker run -p 8080:8080 --env PORT=8080 --name $(LOCAL_CONTAINER_NAME) six-disc-changer

.PHONY: docker-stop
docker-stop:
	-docker stop $(LOCAL_CONTAINER_NAME)
	-docker rm $(LOCAL_CONTAINER_NAME)

.PHONY: fly-deploy
fly-deploy:
	fly deploy

.PHONY: aws-login
aws-login:
	@echo 'aws.login 6-disc-changer.AdministratorAccess'

.PHONY: pulumi-login
pulumi-login:
	cd pulumi && pulumi login 's3://pulumi-state-214549340182?region=us-east-1&awssdk=v2'

.PHONY: pulumi-select
pulumi-select:
	cd pulumi && pulumi stack select $(ENVIRONMENT)

.PHONY: pulumi-refresh
pulumi-refresh: pulumi-select
	cd pulumi && pulumi refresh --yes

.PHONY: pulumi-preview
pulumi-preview: pulumi-select
	cd pulumi && pulumi preview

.PHONY: pulumi-export
pulumi-export: pulumi-select
	cd pulumi && pulumi stack export

.PHONY: pulumi-up
pulumi-up: pulumi-select
	cd pulumi && pulumi up --yes
