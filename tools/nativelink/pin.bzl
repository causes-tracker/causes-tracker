# Single source of truth for the pinned worker layer's content digest,
# shared between tools/nativelink/BUCK (the layer's own validation pin) and
# platforms/BUCK (so bumping this digest also changes the container-image
# property baked into every image_build action, invalidating NativeLink's
# action cache for actions that ran under the old image).
WORKER_LAYER_DIGEST = "e290f8f7a910bb002e88673e72d94ff36f0c4170efbf7a0a439042437fca5b59"
