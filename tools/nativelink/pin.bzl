# Single source of truth for the pinned worker layer's content digest,
# shared between tools/nativelink/BUCK (the layer's own validation pin) and
# platforms/BUCK (so bumping this digest also changes the container-image
# property baked into every image_build action, invalidating NativeLink's
# action cache for actions that ran under the old image).
WORKER_LAYER_DIGEST = "fc1afd47c55bb3a8b26ce67c63ad58c624d184eef2f1f4982b2c7387eb415d77"
