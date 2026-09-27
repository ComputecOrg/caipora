#!/usr/bin/env bash
# Máquinas spot temporárias na AWS (Estocolmo, eu-north-1) para gerar dados da rede.
# Rodar no Git Bash, na raiz do repositório, com `aws login` feito.
#
#   deploy/aws_spot.sh start <tipo> <horas>
#       Sobe uma máquina spot. Ela se desliga, e é apagada, sozinha depois de <horas>.
#   deploy/aws_spot.sh datagen <id> <binário linux> <rede> <prefixo> <semente> <horas>
#       Um `caipora datagen` por núcleo; para sozinho depois de <horas>.
#   deploy/aws_spot.sh fetch <id> <prefixo>
#       Traz os arquivos datagen/<prefixo>-*.txt, compactados.
#   deploy/aws_spot.sh stop <id>
#       Apaga a máquina na hora.
#   deploy/aws_spot.sh list
#       Máquinas do projeto que ainda existem.
#
# Custos: o orçamento "Caipora - computacao" (US$ 18,62/mês) avisa por e-mail a 50/80/100% e na
# previsão. O desligamento automático é a trava contra máquina esquecida.
set -euo pipefail
REGION=${CAIPORA_AWS_REGION:-eu-north-1}
AWS=${AWS_CLI:-"/c/Program Files/Amazon/AWSCLIV2/aws.exe"}
KEY=${CAIPORA_AWS_KEY:-$HOME/.ssh/caipora-aws.pem}
KNOWN=$HOME/.ssh/caipora-aws-known_hosts
# MSYS_NO_PATHCONV: o Git Bash converteria /aws/service/... num caminho do Windows.
aws_() { MSYS_NO_PATHCONV=1 "$AWS" --region "$REGION" "$@"; }
ssh_() {
  local ip=$1; shift
  ssh -i "$KEY" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$KNOWN" \
    -o ConnectTimeout=15 "ubuntu@$ip" "$@"
}
ip_of() {
  aws_ ec2 describe-instances --instance-ids "$1" \
    --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
}

cmd=${1:-}
case "$cmd" in
  start)
    type=$2; hours=$3
    ami=$(aws_ ssm get-parameter \
      --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id \
      --query Parameter.Value --output text)
    sg=$(aws_ ec2 describe-security-groups --filters Name=group-name,Values=caipora-ssh \
      --query 'SecurityGroups[0].GroupId' --output text)
    # A própria máquina agenda o desligamento; com "terminate", desligar = apagar.
    userdata=$(printf '#!/bin/bash\nshutdown -h +%d\n' "$((hours * 60))")
    id=$(aws_ ec2 run-instances --image-id "$ami" --instance-type "$type" --count 1 \
      --key-name caipora --security-group-ids "$sg" \
      --instance-market-options \
        '{"MarketType":"spot","SpotOptions":{"SpotInstanceType":"one-time","InstanceInterruptionBehavior":"terminate"}}' \
      --instance-initiated-shutdown-behavior terminate \
      --user-data "$userdata" \
      --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=caipora-spot},{Key=Project,Value=caipora}]' \
      --query 'Instances[0].InstanceId' --output text)
    aws_ ec2 wait instance-running --instance-ids "$id"
    ip=$(ip_of "$id")
    for _ in $(seq 1 30); do ssh_ "$ip" true 2>/dev/null && break; sleep 5; done
    echo "$id $ip ($type, apaga sozinha em ${hours} h)"
    ;;
  datagen)
    id=$2; bin=$3; net=$4; prefix=$5; seed=$6; hours=$7
    ip=$(ip_of "$id")
    ssh_ "$ip" 'mkdir -p ~/caipora/data'
    scp -q -i "$KEY" -o UserKnownHostsFile="$KNOWN" "$bin" "ubuntu@$ip:caipora/caipora"
    scp -q -i "$KEY" -o UserKnownHostsFile="$KNOWN" "$net" "ubuntu@$ip:caipora/net.nnue"
    ssh_ "$ip" "cd ~/caipora && chmod +x caipora && n=\$(nproc) && for i in \$(seq 0 \$((n - 1))); do
        s=\$(( $seed + i ))
        nohup timeout ${hours}h ./caipora datagen 100000000 data/$prefix-\$s.txt \$s 5000 net.nnue \
          > data/$prefix-\$s.log 2>&1 &
      done; echo \"\$n processos de datagen, sementes $seed a \$(( $seed + n - 1 ))\""
    ;;
  fetch)
    id=$2; prefix=$3
    ip=$(ip_of "$id")
    mkdir -p datagen
    ssh_ "$ip" "cd ~/caipora/data && for f in $prefix-*.txt; do head -n \$(wc -l < \$f) \$f; done | gzip -1" \
      > "datagen/$prefix-aws-$id.txt.gz"
    ls -la "datagen/$prefix-aws-$id.txt.gz"
    echo "posições: $(gzip -dc "datagen/$prefix-aws-$id.txt.gz" | wc -l)"
    ;;
  stop)
    aws_ ec2 terminate-instances --instance-ids "$2" \
      --query 'TerminatingInstances[0].CurrentState.Name' --output text
    ;;
  list)
    aws_ ec2 describe-instances --filters Name=tag:Project,Values=caipora \
      Name=instance-state-name,Values=pending,running,stopping \
      --query 'Reservations[].Instances[].[InstanceId,InstanceType,State.Name,PublicIpAddress,LaunchTime]' \
      --output text
    ;;
  *)
    sed -n '2,20p' "$0"
    exit 1
    ;;
esac
