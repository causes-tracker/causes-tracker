# Single source of truth for the pinned worker layer's content digest,
# shared between tools/nativelink/BUCK (the layer's own validation pin) and
# platforms/BUCK (so bumping this digest also changes the container-image
# property baked into every image_build action, invalidating NativeLink's
# action cache for actions that ran under the old image).
WORKER_LAYER_DIGEST = "6ea801e86a66f81f164da8cd851be53223f305eef77e6f311bc70dad6ab11a2f"
