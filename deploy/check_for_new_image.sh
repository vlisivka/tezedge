#!/bin/bash
# ensure running bash
if ! [ -n "$BASH_VERSION" ];then
    echo "this is not bash, calling self with bash....";
    SCRIPT=$(readlink -f "$0")
    /bin/bash $SCRIPT
    exit;
fi

if [ -z "$1" ]; then
  echo "No tezedge root path specified"
  exit 1
fi

TEZEDGE_PATH="$1/deploy"

OWNER="simplestakingcom"
REPOSITORY="tezedge"
TAG="latest"

LATEST="`curl https://hub.docker.com/v2/repositories/$OWNER/$REPOSITORY/tags/$TAG/?page_size=100 | jq -r '.images|.[]|.digest'`"
LATEST="$OWNER/$REPOSITORY@$LATEST"

RUNNING=`docker inspect "$OWNER/$REPOSITORY:$TAG" | jq -r '.|.[]|.RepoDigests|.[]|.'`

if [ "$RUNNING" == "" ];then
    echo "Not Running!!"
    # Well, make him running!
    ./docker-debugger.sh
    exit;
fi

if [ "$RUNNING" == "$LATEST" ];then
    echo "same, do nothing"
else
    echo "update!"
    echo "$RUNNING != $LATEST"

    cd $TEZEDGE_PATH && \
    # docker-compose -f docker-compose.debugger.yml down && \
    # docker system prune -f -a && \
    # docker volume prune -f && \
    docker-compose -f docker-compose.debugger.yml pull 
    # ./docker-debugger.sh
fi
