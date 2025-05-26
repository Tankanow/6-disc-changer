
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
