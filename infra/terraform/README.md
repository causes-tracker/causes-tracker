# infra/terraform — Aurora Serverless v2

Provisions an Aurora PostgreSQL 16 Serverless v2 cluster.
The only required input is `region`.

## Post-apply: enable pgvector

pgvector is built into Aurora PostgreSQL and needs no `shared_preload_libraries` entry.
Once you have connectivity to the cluster from inside the VPC (e.g. via a bastion or VPN), run:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

## Usage

```sh
bazel run //infra:tofu -- init
bazel run //infra:tofu -- plan
bazel run //infra:tofu -- apply
bazel run //infra:tofu -- destroy
```

## Minimum IAM permissions

Attach these to the role used by the machine or user running OpenTofu.

### Plan (`tofu plan`)

| AWS managed policy | Why |
|---|---|
| `AmazonVPCReadOnlyAccess` | Read AZs, VPCs, subnets, security groups |
| `AmazonRDSReadOnlyAccess` | Read DB subnet groups, clusters, instances, parameter groups |

### Apply and destroy (`tofu apply` / `tofu destroy`)

| AWS managed policy | Why |
|---|---|
| `AmazonVPCFullAccess` | Create/delete VPC, subnets, security groups |
| `AmazonRDSFullAccess` | Create/delete DB subnet groups, clusters, instances, parameter groups; includes `iam:CreateServiceLinkedRole` for `rds.amazonaws.com` |

`AmazonRDSFullAccess` does not grant Secrets Manager access.
`manage_master_user_password = true` causes RDS to create and manage a secret on the caller's behalf.
Attach this inline policy:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "ManageRdsManagedSecret",
      "Effect": "Allow",
      "Action": [
        "secretsmanager:CreateSecret",
        "secretsmanager:TagResource",
        "secretsmanager:DescribeSecret",
        "secretsmanager:DeleteSecret"
      ],
      "Resource": "arn:aws:secretsmanager:*:*:secret:rds!cluster-*"
    }
  ]
}
```

RDS-managed secrets always follow the `rds!cluster-` naming prefix, so this policy is scoped as tightly as possible.

### First-time setup note

If no Aurora cluster has ever been created in the AWS account, the RDS service-linked role (`AWSServiceRoleForRDS`) may not exist yet.
`AmazonRDSFullAccess` includes the `iam:CreateServiceLinkedRole` permission scoped to `rds.amazonaws.com`, so the first `tofu apply` will create it automatically.
