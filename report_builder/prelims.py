# Preliminaries and Title Page Definition

TITLE_PAGE_MD = """# REPORT ON STUDENT INDUSTRIAL WORK EXPERIENCE SCHEME (SIWES)
## EXPERIENCES AND TECHNICAL PROJECT UNDERTAKEN AT
# AKWA IBOM STATE INTERNAL REVENUE SERVICE (AKIRS)
### (JOINT REVENUE BOARD DIGITAL INFRASTRUCTURE PROGRAMME)
### REVENUE HOUSE, UYO, AKWA IBOM STATE

---

### ON

# BRIDGE: A HIGH-PERFORMANCE FAULT-TOLERANT DISTRIBUTED INGRESS ROUTING MESH AND MULTI-VM GOSSIP NETWORK

---

### BY

**BEN, PENIEL**  
**REG NO:** [MATRICULATION NUMBER: 21/SC/CO/XXXX]  
**DEPARTMENT OF COMPUTER SCIENCE**  
**FACULTY OF COMPUTING**  
**UNIVERSITY OF UYO, UYO, AKWA IBOM STATE**  

**COURSE CODE: CSC 329 (SIWES)**

---

### SUBMITTED TO:
**DEPARTMENT OF COMPUTER SCIENCE**  
**FACULTY OF COMPUTING**  
**UNIVERSITY OF UYO, UYO, AKWA IBOM STATE**  

*IN PARTIAL FULFILLMENT OF THE REQUIREMENTS FOR THE AWARD OF THE DEGREE OF BACHELOR OF SCIENCE (B.SC.) IN COMPUTER SCIENCE*

**OCTOBER, 2026**

<div style="page-break-after: always;"></div>

---

# TABLE OF CONTENTS

- **Title Page** .................................................................................................... i
- **Table of Contents** ............................................................................................. ii

### CHAPTER ONE (1): BRIEF HISTORY OF SIWES
- **1.0 Introduction** .............................................................................................. 1
- **1.2 Brief History of SIWES in Nigeria** ................................................................ 1
- **1.3 Vision Statement of SIWES** ............................................................................ 2
- **1.4 Mission Statement of SIWES** ......................................................................... 2
- **1.5 Aims of SIWES** ........................................................................................... 3
- **1.6 Objectives of SIWES** .................................................................................... 3

### CHAPTER TWO (2): BRIEF HISTORY OF THE SIWES ORGANIZATION (AKIRS)
- **2.0 Introduction** .............................................................................................. 4
- **2.1 About AKIRS** ............................................................................................. 4
- **2.2 Current Modernization and Technology Programmes** ........................................... 5
  - 2.2.1 Joint Revenue Board (JRB) Digital Infrastructure Programme ............................... 5
  - 2.2.2 Datacenter Infrastructure Node and Subnational Tax Modernization ...................... 5
  - 2.2.3 Electronic Tax Intelligence Gathering and Automated Compliance Systems ............. 6
- **2.3 Mandate of AKIRS** ...................................................................................... 6
- **2.4 Vision of AKIRS** ......................................................................................... 7
- **2.5 Mission of AKIRS** ........................................................................................ 7
- **2.6 Core Values of AKIRS** ................................................................................. 7
- **2.7 Organizational Structure and Directorate of ICT & Tax Intelligence** .......................... 7

### CHAPTER THREE (3): EXPERIENCE GAINED AND WORK DONE
- **3.0 Introduction to AKIRS Environment and Engineering Units** .................................. 9
- **3.1 Overview of Experiences Gained** .................................................................... 9
- **3.2 Network Infrastructure & Security Administration** ............................................... 10
  - 3.2.1 Sophos XGS 138 Next-Generation Firewall Management ................................... 11
  - 3.2.2 Cisco Catalyst 1300 Switch Administration & VLAN Segmentation ....................... 11
  - 3.2.3 High-Availability Internet Connectivity & Starlink Satellite Link ........................... 12
  - 3.2.4 Tasks Completed in Network Administration .................................................. 12
- **3.3 Server & Hypervisor Administration** .............................................................. 12
  - 3.3.1 Dual HPE ProLiant DL380 Gen11 Compute Servers .......................................... 13
  - 3.3.2 VMware ESXi 8.0 Hypervisor Host Management ............................................... 13
  - 3.3.3 VMware vCenter Server Administration & Resource Scheduling ............................ 13
  - 3.3.4 Virtual Machine Lifecycle & Live Migration (vMotion) ........................................ 14
  - 3.3.5 Tasks Completed in Server & Hypervisor Administration .................................... 14
- **3.4 Enterprise Storage, Backup & Disaster Recovery** ................................................. 15
  - 3.4.1 Synology DSM NAS Administration & RAID 5 Storage Architecture ..................... 15
  - 3.4.2 Veeam Backup & Replication Configuration for Mission-Critical Tax Systems ........... 16
  - 3.4.3 Tasks Completed in Storage & Disaster Recovery .............................................. 16
- **3.5 Automated Taxpayer Discovery & OSINT Reconnaissance (akirs-auto)** ....................... 16
  - 3.5.1 Background and Objectives in Akwa Ibom State Commercial Mapping .................... 17
  - 3.5.2 Playwright Scraping of Facebook Ads Library for Commercial Dialects & Geo-targets 17
  - 3.5.3 Multi-Tier Reconnaissance Pipeline (OSM Nominatim, Hunter.io, TomTom Places) .... 18
  - 3.5.4 Tasks Completed in Tax Intelligence Automation ............................................. 18
- **3.6 Taxpayer Data Cleaning, Deduplication & Batch ETL Pipelines (akirs-data-cleaner)** .... 18
  - 3.6.1 Batch Processing of Commercial Bank New Account Feeds .................................. 19
  - 3.6.2 FastAPI-Powered Ingestion Portal & OpenPyXL Parsing Engine ............................ 19
  - 3.6.3 Algorithmic NUBAN & TIN Validation, Duplicate Resolution, and Branch Filtering ... 20
  - 3.6.4 Tasks Completed in Data Cleaning & Normalization .......................................... 20
- **3.7 Monitoring, Alerting & Hardware Fleet Maintenance** ........................................... 20
  - 3.7.1 Hardware Health Audits & Proactive Alerting .................................................. 20
  - 3.7.2 HP ProBook Engineering Fleet Provisioning & Endpoint Security .......................... 21
- **3.8 Version Control, DevOps & Containerization** ...................................................... 21
  - 3.8.1 Git, GitHub & Distributed Code Collaboration .................................................. 21
  - 3.8.2 Docker Containerization & Microservice Environments ....................................... 21
- **3.9 Soft Skills, Cross-Disciplinary Collaboration & Technical Communication** ................ 21
- **3.10 Chapter Three Conclusion** .......................................................................... 22

### CHAPTER FOUR (4): TECHNICAL PROJECT — BRIDGE (FAULT-TOLERANT DISTRIBUTED INGRESS ROUTING MESH & MULTI-VM GOSSIP NETWORK)
- **4.0 Introduction** .............................................................................................. 23
- **4.1 Background** ............................................................................................... 23
- **4.2 Problem Statement** ..................................................................................... 24
- **4.3 Integrated Development Environment (IDE) & Systems Tooling** ............................. 25
  - 4.3.1 Systems Programming with Rust .................................................................. 26
  - 4.3.2 Tokio Asynchronous Event-Driven Runtime ..................................................... 26
  - 4.3.3 Development Toolchain: Cargo, Linux Namespaces, Wireshark, VS Code ................. 26
- **4.4 Methodology & Architectural Design** .............................................................. 26
  - 4.4.1 High-Level Distributed Ingress Routing Mesh Architecture ................................. 27
  - 4.4.2 Mode 1: Direct Mode (L7 Reverse Proxy with Automated ACME TLS Termination) .... 27
  - 4.4.3 Mode 2: Handoff Mode (L4 SNI-Based Zero-TLS-Termination TCP Stream Splicing) ... 28
  - 4.4.4 Mode 3: Managed Mode (Control-Plane Orchestration for Node-Level Proxies) ......... 29
  - 4.4.5 Lock-Free In-Memory Domain Registry & Route Resolution ................................. 30
- **4.5 Cluster Coordination & Fault Tolerance** .......................................................... 30
  - 4.5.1 UDP-based Peer Gossip Protocol & Heartbeat Topology .................................... 30
  - 4.5.2 Dynamic Failure Detection, Node Eviction & Self-Healing ................................... 30
  - 4.5.3 Graceful Shutdown Protocol and Zero-Downtime Session Handoff .......................... 31
- **4.6 Verification, Testing & Performance Evaluation** ................................................. 31
  - 4.6.1 Comprehensive Integration & Concurrency Test Suite ....................................... 31
  - 4.6.2 L4 Handoff vs L7 Direct Latency & Throughput Benchmark Analysis ..................... 32
  - 4.6.3 Fault-Injection, Node Failure & Dynamic Failover Simulation .............................. 33
- **4.7 Key Results and Architectural Insights** ........................................................... 33
- **4.8 Conclusion** ................................................................................................ 33
- **4.9 References** ................................................................................................. 34

<div style="page-break-after: always;"></div>
"""
