terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.0"
    }
  }
}

provider "aws" {
  region = var.region
}

data "aws_availability_zones" "available" {
  state = "available"
}

# ── Networking ────────────────────────────────────────────────────────────────

resource "aws_vpc" "causes" {
  cidr_block = "10.0.0.0/16"
  tags       = { Name = "causes" }
}

resource "aws_subnet" "causes_a" {
  vpc_id            = aws_vpc.causes.id
  cidr_block        = "10.0.1.0/24"
  availability_zone = data.aws_availability_zones.available.names[0]
  tags              = { Name = "causes-a" }
}

resource "aws_subnet" "causes_b" {
  vpc_id            = aws_vpc.causes.id
  cidr_block        = "10.0.2.0/24"
  availability_zone = data.aws_availability_zones.available.names[1]
  tags              = { Name = "causes-b" }
}

resource "aws_security_group" "causes_db" {
  name   = "causes-db"
  vpc_id = aws_vpc.causes.id

  ingress {
    description = "PostgreSQL from within the VPC"
    from_port   = 5432
    to_port     = 5432
    protocol    = "tcp"
    cidr_blocks = [aws_vpc.causes.cidr_block]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_db_subnet_group" "causes" {
  name       = "causes"
  subnet_ids = [aws_subnet.causes_a.id, aws_subnet.causes_b.id]
}

# ── Aurora Serverless v2 ──────────────────────────────────────────────────────

resource "aws_rds_cluster" "causes" {
  cluster_identifier          = "causes"
  engine                      = "aurora-postgresql"
  engine_version              = "16.6"
  database_name               = "causes"
  master_username             = "causes"
  manage_master_user_password = true
  db_subnet_group_name        = aws_db_subnet_group.causes.name
  vpc_security_group_ids      = [aws_security_group.causes_db.id]
  skip_final_snapshot         = true

  serverlessv2_scaling_configuration {
    min_capacity             = 0
    max_capacity             = 8
    seconds_until_auto_pause = 300
  }
}

resource "aws_rds_cluster_instance" "causes" {
  cluster_identifier = aws_rds_cluster.causes.id
  instance_class     = "db.serverless"
  engine             = aws_rds_cluster.causes.engine
  engine_version     = aws_rds_cluster.causes.engine_version
}
