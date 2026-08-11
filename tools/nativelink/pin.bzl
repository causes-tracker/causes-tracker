# Single source of truth for the pinned worker layer's content digest,
# shared between tools/nativelink/BUCK (the layer's own validation pin) and
# platforms/BUCK (so bumping this digest also changes the container-image
# property baked into every image_build action, invalidating NativeLink's
# action cache for actions that ran under the old image).
WORKER_LAYER_DIGEST = "f68d650526f3a4ddfe231e4307015d1c3477cfc3974ac5bb4555b856784395ec"
