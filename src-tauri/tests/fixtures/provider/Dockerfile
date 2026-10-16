# Prepare separately on an explicitly dedicated engine. The contract run never
# pulls an image or builds implicitly. Pass an already-local immutable base ID.
ARG BASE_IMAGE
FROM ${BASE_IMAGE}
LABEL io.colimaui.fixture="winui-v1"
CMD ["sleep", "600"]
