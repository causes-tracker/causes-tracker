# Single source of truth for the pinned worker layer's content digest,
# shared between tools/nativelink/BUCK (the layer's own validation pin) and
# platforms/BUCK (so bumping this digest also changes the container-image
# property baked into every image_build action, invalidating NativeLink's
# action cache for actions that ran under the old image).
WORKER_LAYER_DIGEST = "7917a34dce0bbd54ed1831c161409a6da234fcf3da32e255ddfb8ba654e7e098"
