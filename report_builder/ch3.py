# Chapter 3: Experience Gained and Work Done

CHAPTER_THREE_MD = """# CHAPTER THREE (3)
## EXPERIENCE GAINED AND WORK DONE

### 3.0 INTRODUCTION TO AKIRS ENVIRONMENT AND ENGINEERING UNITS
At the commencement of the six-month Student Industrial Work Experience Scheme (SIWES) at the Akwa Ibom State Internal Revenue Service (AKIRS), I was integrated into the **Directorate of Information and Communication Technology (ICT) and Tax Intelligence**. This directorate functions as the technical backbone of the state's revenue mobilization architecture, responsible for maintaining continuous service uptime, securing sensitive financial data against cyber threats, managing enterprise datacenter systems, and developing software automation solutions to resolve operational bottlenecks.

The engineering operations of the directorate are organized into specialized yet deeply interconnected functional units:
1. **Datacenter & Virtualization Operations Unit:** Custodian of the on-premise physical infrastructure deployed under the Joint Revenue Board (JRB) Digital Infrastructure Programme, including dual HPE ProLiant Gen11 server blades, VMware vSphere/ESXi hypervisors, and vCenter management clusters.
2. **Network Engineering & Perimeter Security Unit:** Responsible for enterprise connectivity, firewall rule configurations on Sophos XGS 138 appliances, Cisco Catalyst layer-2/3 switching fabrics, IEEE 802.1Q VLAN segmentation, and redundant WAN gateways (Starlink LEO satellite and terrestrial fiber).
3. **Storage & Disaster Recovery Unit:** Oversees the enterprise Synology Network Attached Storage (NAS) fabrics configured in RAID 5, NVMe SSD cache layers, and the enterprise Veeam Backup & Replication platform protecting production virtual machines.
4. **Data Systems & Software Automation Unit:** Develops custom software automation scripts, data cleaning pipelines, and intelligence-gathering tools to convert unstructured commercial data into structured, actionable tax intelligence.

Understanding the structural interplay of these units provided a holistic foundation for my internship, bridging theoretical knowledge of operating systems, networking, database architecture, and software design with high-stakes production execution.

---

### 3.1 OVERVIEW OF EXPERIENCES GAINED
The industrial attachment at AKIRS proved to be an intensely transformative and technically expansive phase in my academic career. Moving from isolated laboratory exercises and theoretical classroom lectures into a live enterprise datacenter managing sensitive financial records for an entire state presented a steep but immensely rewarding learning curve.

Key areas of hands-on technical competencies acquired during the period include:
* **Enterprise Infrastructure & Virtualization:** Gained deep operational proficiency in provisioning, configuring, and monitoring type-1 bare-metal hypervisors (VMware ESXi 8.0) managed through VMware vCenter Server Appliance (VCSA). Mastered virtual machine lifecycle management, dynamic resource allocation (vCPU/RAM scheduling), virtual network switches (vSwitches), and zero-downtime live migrations (vMotion).
* **Next-Generation Network Security & Routing:** Developed hands-on competence in configuring Sophos XGS 138 firewalls, implementing granular firewall rules, Network Address Translation (NAT), Intrusion Prevention System (IPS) policies, deep packet inspection (DPI), SSL/TLS inspection, and site-to-site IPsec/SSL VPN tunnels. Administered Cisco Catalyst 1300 enterprise managed switches, configuring VLAN segregation, port security, and 802.1Q trunking.
* **Storage Engineering & Disaster Recovery:** Mastered the management of Synology DSM NAS appliances operating under RAID 5 architecture with NVMe SSD caching. Implemented enterprise-grade disaster recovery workflows using Veeam Backup & Replication, executing automated incremental VM backups, synthetic full backups, retention policy scheduling, and simulated instant VM recovery drills.
* **Asynchronous Web Scraping & Tax Intelligence Automation:** Conceptualized, engineered, and deployed `akirs-auto`, an asynchronous Python and Playwright pipeline that scrapes the Meta/Facebook Ads Library for commercial activity across Akwa Ibom State, enriching discovered advertisers with contact identifiers via multi-tier OSINT APIs (OpenStreetMap Nominatim, Hunter.io, TomTom Places) to identify unregistered commercial enterprises.
* **Batch Data Cleaning & ETL Pipeline Development:** Developed and maintained `akirs-data-cleaner`, a production-grade FastAPI and OpenPyXL web portal designed to ingest heterogeneous monthly commercial bank account spreadsheets, execute algorithmic 10-digit NUBAN checksum validations, verify Taxpayer Identification Numbers (TIN), perform deduplication, and filter records by geographic tax jurisdictions.
* **DevOps, Containerization & Systems Monitoring:** Mastered the Linux command-line environment across Ubuntu and Debian distributions, containerized Python web services using multi-stage Dockerfiles, orchestrated services via Docker Compose, and utilized Git and GitHub for distributed version control.
* **Professional & Soft Skills Development:** Acquired essential professional competencies, including technical documentation writing, agile project coordination, public technical presentations during internal sprint reviews, and cross-functional communication with non-technical tax auditors and legal enforcement officers.

---

### 3.2 NETWORK INFRASTRUCTURE & SECURITY ADMINISTRATION

```
                         ┌───────────────────────────────────────────────┐
                         │              INTERNET CONNECTIVITY            │
                         │    [Terrestrial Fiber]  +  [Starlink LEO]     │
                         └───────────────────────┬───────────────────────┘
                                                 │ Multi-WAN Failover
                                                 ▼
                         ┌───────────────────────────────────────────────┐
                         │         SOPHOS XGS 138 FIREWALL (NGFW)        │
                         │     Stateful Inspection · NAT · IPS · DPI     │
                         └───────────────────────┬───────────────────────┘
                                                 │ 802.1Q Trunk
                                                 ▼
                         ┌───────────────────────────────────────────────┐
                         │          CISCO CATALYST 1300 SWITCH           │
                         │   VLAN 10: Management · VLAN 20: Servers      │
                         │   VLAN 30: Staff LAN   · VLAN 40: DMZ/Public   │
                         └──────────┬─────────────────────────┬──────────┘
                                    │                         │
                  ┌─────────────────┴─────────┐     ┌─────────┴─────────────────┐
                  ▼                           ▼     ▼                           ▼
         [HPE DL380 Gen11 Node 1]   [HPE DL380 Node 2]   [Synology NAS]   [Staff Workstations]
```

#### 3.2.1 Sophos XGS 138 Next-Generation Firewall Management
The network perimeter of the AKIRS local datacenter node is anchored by a Sophos XGS 138 Next-Generation Firewall running SFOS (Sophos Firewall OS). The appliance provides dual-engine architecture combining a multi-core x86 CPU with an integrated Xstream Flow Processor designed for dedicated hardware acceleration of TLS inspection, IPS packet processing, and fast-path routing.

During my attachment, I assisted in:
* **Firewall Rule Formulation:** Implementing strict zone-to-zone access policies (WAN to DMZ, LAN to WAN, DMZ to LAN) adhering to the principle of least privilege.
* **Network Address Translation (NAT):** Formulating Source NAT (SNAT) masquerading rules for internal subnets and Destination NAT (DNAT) port-forwarding rules to expose specific internal tax portals securely.
* **Intrusion Prevention System (IPS):** Assigning custom IPS policy rule sets optimized to detect, log, and drop known network vulnerability exploits, malicious port scanning, and denial-of-service signatures.
* **SSL/TLS Deep Packet Inspection:** Configuring certificate authorities and decrypting outbound HTTPS traffic to inspect for concealed malicious payloads and unauthorized data exfiltration.
* **Configuration Backup Administration:** Scheduling automated daily encrypted configuration snapshots exported to secure offsite storage, facilitating rapid bare-metal recovery in the event of hardware failure.

#### 3.2.2 Cisco Catalyst 1300 Switch Administration & VLAN Segmentation
Core distribution and access layer switching in the datacenter is handled by enterprise-grade Cisco Catalyst 1300 managed switches. To prevent broadcast storms, isolate administrative access, and protect production database servers from workstation subnets, a rigorous IEEE 802.1Q Virtual Local Area Network (VLAN) scheme was implemented:
* **VLAN 10 (Management Subnet):** Dedicated strictly to out-of-band management interfaces, including HPE iLO 6, VMware ESXi management vmkernel ports, and switch console web interfaces.
* **VLAN 20 (Server & Virtualization Subnet):** Reserved exclusively for production virtual machines hosting tax databases, application backends, and domain controllers.
* **VLAN 30 (Staff Workstation Subnet):** Serving desktop clients, printers, and administrative laptops across the Revenue House floors with strict firewall access rules preventing direct connections to database ports.
* **VLAN 40 (DMZ Subnet):** Segmented perimeter network hosting externally accessible web portals and staging ingress points.

Port security policies were enforced on physical switch ports, binding connected devices to authorized MAC addresses and immediately disabling ports (err-disable state) upon detecting unauthorized network hardware insertion.

#### 3.2.3 High-Availability Internet Connectivity & Starlink Satellite Link
Revenue collection operations require continuous, uninterrupted connectivity to commercial banks, payment settlement switches (such as Interswitch and Remita), and national tax databases. To mitigate the chronic risk of terrestrial fiber cuts caused by road construction or municipal utility works, a resilient dual-WAN failover architecture was deployed.

The primary WAN link comprises a dedicated enterprise terrestrial fiber line terminating in Port 2 of the Sophos XGS firewall. The secondary redundant WAN link is provided by a **Starlink Low-Earth Orbit (LEO) Satellite Terminal** mounted on the roof of Revenue House, connected via an enterprise PoE injector to Port 3. The Sophos firewall executes automated ICMP health probing every five (5) seconds across both interfaces. When the primary fiber link drops packets or experiences latency exceeding 250ms, the firewall automatically triggers zero-packet-loss session failover to the Starlink satellite link, ensuring field tax offices and bank integrations continue operating seamlessly.

#### 3.2.4 Tasks Completed in Network Administration
* Configured and audited over 40 granular firewall rule sets on the Sophos XGS 138 console, disabling legacy permissive rules and establishing strict time-based internet access policies for administrative staff.
* Configured IEEE 802.1Q VLAN trunk ports and access ports across the Cisco Catalyst 1300 switch, verifying subnet isolation using ping sweeps and Wireshark packet analysis.
* Conducted weekly failover simulation tests by physically disconnecting the primary terrestrial WAN link and verifying that the Sophos multi-WAN engine successfully diverted all active production traffic to Starlink within three seconds.
* Generated and archived automated weekly firewall configuration backups and analyzed threat intelligence dashboards to block unauthorized foreign IP ranges probing the state perimeter.

---

### 3.3 SERVER & HYPERVISOR ADMINISTRATION

```
                         ┌───────────────────────────────────────────────┐
                         │        VMWARE VCENTER SERVER APPLIANCE        │
                         │   Centralized Cluster Management & vMotion    │
                         └───────────────────────┬───────────────────────┘
                                                 │
                     ┌───────────────────────────┴───────────────────────────┐
                     ▼                                                       ▼
      ┌─────────────────────────────┐                         ┌─────────────────────────────┐
      │   HPE PROLIANT DL380 GEN11  │                         │   HPE PROLIANT DL380 GEN11  │
      │         [HOST NODE 1]       │  ◄─── Live vMotion ───► │         [HOST NODE 2]       │
      │  VMware ESXi 8.0 Hypervisor │     Migration Link      │  VMware ESXi 8.0 Hypervisor │
      ├─────────────────────────────┤                         ├─────────────────────────────┤
      │ [VM1: Tax Core Database]    │                         │ [VM4: Intelligence Recon]   │
      │ [VM2: Bank Ingestion Portal]│                         │ [VM5: Ingress Proxy Mesh]   │
      │ [VM3: Active Directory/DNS] │                         │ [VM6: Veeam Backup Server]  │
      └─────────────────────────────┘                         └─────────────────────────────┘
```

#### 3.3.1 Dual HPE ProLiant DL380 Gen11 Compute Servers
The local datacenter node hosts two high-density enterprise HPE ProLiant DL380 Gen11 2U rack servers. Each server chassis is equipped with dual Intel Xeon Silver Scalable processors, 128 GB of high-speed DDR5 Registered ECC memory, redundant hot-plug platinum power supplies (800W), and integrated HPE Integrated Lights-Out (iLO 6) management processors.

I was exposed to physical server mounting, cable dress management, out-of-band iLO configuration, firmware updating, and thermal threshold monitoring. The hardware provides the foundational computational capacity to run mission-critical virtualized database servers and application services without resource contention.

#### 3.3.2 VMware ESXi 8.0 Hypervisor Host Management
Each physical HPE compute server runs a bare-metal installation of VMware ESXi 8.0. As a type-1 hypervisor, ESXi runs directly on the bare-metal hardware without an underlying host operating system, ensuring optimal performance, near-zero virtualization overhead, and rigorous hardware isolation.

Key hypervisor administration tasks performed included:
* **Storage Datastore Mounting:** Configuring iSCSI and NFS software adapters to connect ESXi hosts to shared storage volumes hosted on the Synology NAS.
* **Virtual Switch (vSwitch) Configuration:** Creating standard virtual switches (vSwitch0, vSwitch1), provisioning virtual port groups tagged with respective VLAN IDs (VLAN 10, 20, 30, 40), and assigning physical network interface cards (NIC teaming) for failover and load balancing.
* **Hardware Resource Monitoring:** Tracking CPU clock utilization, memory ballooning, storage I/O latency, and hardware sensor telemetry directly through the ESXi Host Client web interface.

#### 3.3.3 VMware vCenter Server Administration & Resource Scheduling
Both physical ESXi hosts were joined into a unified enterprise cluster managed through the **VMware vCenter Server Appliance (VCSA)**. vCenter provides a centralized single-pane-of-glass management console for orchestrating the entire virtualization fabric.

I was trained on:
* Setting up VMware High Availability (HA) clusters that automatically restart virtual machines on the surviving host if a physical hardware failure occurs on one node.
* Configuring Distributed Resource Scheduler (DRS) rules, balancing computational load dynamically across both physical hosts based on real-time CPU and memory utilization thresholds.
* Organizing virtual machines into structured hierarchical folders and assigning granular Role-Based Access Controls (RBAC) to ensure unauthorized administrators cannot modify core database VM configurations.

#### 3.3.4 Virtual Machine Lifecycle & Live Migration (vMotion)
The datacenter environment runs six primary production virtual machines distributed across the compute cluster:
1. **AKIRS-DB-PROD:** Production relational database engine (PostgreSQL/SQL Server) housing core taxpayer registration records and payment receipts.
2. **AKIRS-DATA-CLEANER:** Linux VM hosting the FastAPI banking data cleaning and batch ingestion engine.
3. **AKIRS-AUTO-RECON:** High-performance Linux VM executing headless Playwright browser automation and OSINT reconnaissance scrapers.
4. **AKIRS-AD-DNS:** Windows Server VM providing enterprise Active Directory Domain Services, Kerberos authentication, and local recursive DNS.
5. **AKIRS-INGRESS-MESH:** System VM running the distributed Bridge ingress proxy nodes for secure internal routing.
6. **AKIRS-VEEAM-B&R:** Windows VM running the Veeam Backup & Replication console and repository controller.

A critical milestone during my attachment was mastering **VMware vMotion**. I successfully performed live migrations of running virtual machines from Host 1 to Host 2 during planned hardware firmware patching cycles. Because vMotion copies active memory states over a dedicated gigabit vMotion VMkernel network while the VM continues processing, zero transaction downtime or connection loss was experienced by operational staff.

#### 3.3.5 Tasks Completed in Server & Hypervisor Administration
* Successfully provisioned, hardened, and deployed four new Debian 12 and Windows Server 2022 virtual machine instances, assigning static IP configurations, installing VMware Tools, and establishing disk quotas.
* Configured VMware vSwitch port groups and mapped them to physical NIC teamed uplinks, ensuring full physical redundancy against cable failure.
* Conducted scheduled zero-downtime vMotion live migrations of active virtual machines during host maintenance windows, verifying zero dropped packets on live application ping streams.
* Monitored host hardware health through HPE iLO 6 consoles, verifying power supply redundancy, CPU core temperature profiles, and fan curve behavior.

---

### 3.4 ENTERPRISE STORAGE, BACKUP & DISASTER RECOVERY

```
                                      ┌─────────────────────────────────┐
                                      │    VMWARE ESXi COMPUTE HOSTS    │
                                      │     Active Production VMs       │
                                      └────────────────┬────────────────┘
                                                       │
                                  Application-Aware    │ Snapshot Stream
                                  Incremental Backup   │ (Every 6 Hours)
                                                       ▼
                                      ┌─────────────────────────────────┐
                                      │    VEEAM BACKUP & REPLICATION   │
                                      │   Deduplication & Compression   │
                                      └────────────────┬────────────────┘
                                                       │
                                    Encrypted iSCSI /  │ SMB Target
                                    NFS Write Stream   │
                                                       ▼
                                      ┌─────────────────────────────────┐
                                      │    SYNOLOGY ENTERPRISE NAS      │
                                      │   RAID 5 Storage Array + SSD    │
                                      │   Automated Offsite Cloud Sync  │
                                      └─────────────────────────────────┘
```

#### 3.4.1 Synology DSM NAS Administration & RAID 5 Storage Architecture
Datacenter storage capacity is anchored by an enterprise Synology RackStation NAS powered by DiskStation Manager (DSM). The storage appliance is populated with high-capacity enterprise SATA hard disk drives formatted in a **RAID 5 (Redundant Array of Independent Disks)** configuration, supplemented by a dual-NVMe M.2 SSD read/write acceleration cache layer.

The RAID 5 configuration distributes parity block calculations across all member drives alongside data blocks:
$$\text{Usable Capacity} = (N - 1) \times S_{\text{smallest}}$$
where $N$ is the total number of drives and $S$ is drive capacity. This architecture allows the storage array to sustain the total physical failure of any single hard drive without data corruption or service interruption. The integrated NVMe SSD cache intercepts high-frequency random small-block read and write requests, dramatically reducing I/O latency for virtual machine datastores and database queries.

#### 3.4.2 Veeam Backup & Replication Configuration for Mission-Critical Tax Systems
Data loss in a state revenue service can cause catastrophic legal, fiscal, and institutional repercussions. To safeguard the state's fiscal records, the **Veeam Backup & Replication** platform was configured to orchestrate enterprise disaster recovery following the industry-standard **3-2-1 Backup Rule**:
* **3 Copies of Data:** One primary production copy, one local backup copy on the Synology NAS, and one offsite/cloud replica.
* **2 Different Media Types:** Enterprise server SAS storage and Synology network-attached RAID storage.
* **1 Copy Kept Offsite:** Encrypted replication to an offsite secure cloud vault.

Key workflows administered included:
* **Application-Aware Image Backups:** Utilizing VMware VSS (Volume Shadow Copy Service) integration to ensure database transactions (PostgreSQL and Microsoft SQL Server) were cleanly committed and quiesced prior to snapshot creation, eliminating corrupted database states.
* **Forward Incremental Backup Schedules:** Executing lightweight incremental backups every six (6) hours with automated synthetic full backups synthesized on Sunday midnights, conserving network bandwidth and storage capacity.
* **Instant VM Recovery Testing:** Verifying disaster recovery readiness by mounting backup VM image files directly from the compressed, deduplicated backup repository on the Synology NAS into the ESXi hypervisor, booting a functional replacement VM in under two minutes without waiting for full disk restoration.

#### 3.4.3 Tasks Completed in Storage & Disaster Recovery
* Administered the Synology DSM Storage Manager, monitoring S.M.A.R.T. health diagnostics across all disk drives, tracking NVMe cache hit ratios (averaging 94.2%), and expanding storage volume allocations.
* Configured and scheduled five daily automated Veeam backup jobs covering all core virtual machine workloads, verifying 100% successful completion status reports each morning.
* Successfully executed a controlled Disaster Recovery Simulation drill, performing an Instant VM Recovery of the staging database server and confirming data integrity and transaction log consistency.
* Implemented AES-256 bit encryption keys on all backup repositories and configured automated email alerting for backup threshold violations.

---

### 3.5 AUTOMATED TAXPAYER DISCOVERY & OSINT RECONNAISSANCE (`akirs-auto`)

```
   ┌───────────────────────┐
   │ META/FACEBOOK ADS LIB │
   │  Akwa Ibom Targeted   │
   └───────────┬───────────┘
               │ Asynchronous Scraping (Playwright Headless)
               ▼
   ┌───────────────────────┐
   │   DISCOVERY ENGINE    │ ───► Extract Ad ID, Advertiser Page, Creative Text, Social Handles
   └───────────┬───────────┘
               │ Raw Entity Stream
               ▼
   ┌─────────────────────────────────────────────────────────────┐
   │             THREE-TIER ENRICHMENT PIPELINE                  │
   ├─────────────────────────────────────────────────────────────┤
   │ Tier 1: Website Scraping (Landing Page emails, phones, regex)│
   │ Tier 2: OpenStreetMap Nominatim + DuckDuckGo + Social Bios  │
   │ Tier 3: TomTom Places POI + Hunter.io / Apollo.io APIs      │
   └─────────────────────────────┬───────────────────────────────┘
                                 │ Clean Structured Data
                                 ▼
   ┌─────────────────────────────────────────────────────────────┐
   │                STANDARDIZED CSV AUDIT EXPORT                │
   │ (Ad ID, Business Name, Phones, Physical Address, LGAs, Tax) │
   └─────────────────────────────────────────────────────────────┘
```

#### 3.5.1 Background and Objectives in Akwa Ibom State Commercial Mapping
A major challenge facing revenue mobilization in Akwa Ibom State is the massive proliferation of informal digital commerce. Hundreds of lucrative enterprises—ranging from boutique retail outlets, restaurants, private medical clinics, and creative agencies to logistics providers—advertise aggressively on digital platforms like Facebook and Instagram but operate without registering with AKIRS or remitting statutory taxes.

Manual field discovery by tax assessment officers is slow, resource-intensive, and geographically restricted. To overcome this limitation, I conceptualized and developed **`akirs-auto`**, an end-to-end automated OSINT and web reconnaissance platform engineered to scan the digital commercial footprint across the state, resolve real-world business identities, and export verified leads to field enforcement officers.

#### 3.5.2 Playwright Scraping of Facebook Ads Library for Commercial Dialects & Geo-targets
The discovery engine leverages **Python** and **Playwright** (running headless Chromium) to programmatically interrogate the Meta/Facebook Ads Library without triggering bot detection or IP throttling:
* **Targeted Geo-Querying:** The scraper targets all thirty-one (31) Local Government Areas of Akwa Ibom State, with intense algorithmic focus on commercial hubs including Uyo, Eket, Ikot Ekpene, Oron, and Abak.
* **Linguistic & Commercial Keyword Heuristics:** To isolate indigenous businesses, the scraper evaluates localized keywords, landmarks (e.g., *Ibom Tropicana, Plaza, Shelter Afrique, Ring Road, Ewet Housing*), local dialects, and commercial purchase intent phrases (*"DM to order", "Pay on delivery", "Visit our showroom"*).
* **Ad Metadata Extraction:** Captures unique Meta Ad IDs, advertised Facebook page URLs, advertised landing page domains, advertisement text, and creation timestamps.

#### 3.5.3 Multi-Tier Reconnaissance Pipeline
Discovered advertisers often hide behind vague brand monikers. To resolve their statutory tax identities, `akirs-auto` executes a cascading three-tier reconnaissance pipeline:
* **Tier 1 (Free On-Page Extraction):** Fetches the business's linked website or landing page, executing high-precision regular expressions to extract corporate email addresses, Nigerian GSM phone numbers (e.g., `+234...`, `080...`, `081...`), and physical street address blocks.
* **Tier 2 (Open Intelligence & Geocoding):** Queries the **OpenStreetMap (OSM) Nominatim API** to resolve fuzzy physical landmarks into standardized geographic coordinates and verified street names within Akwa Ibom State. Leverages asynchronous search engine querying and Instagram bio scraping to accumulate secondary mentions.
* **Tier 3 (Enterprise Verification APIs):** When API keys are supplied, the engine queries **TomTom Places** to match physical storefront locations, points of interest (POI), and verified commercial landlines. Corporate domain names are submitted to **Hunter.io** and **Apollo.io** to extract registered executive directors and legal business names.

The pipeline applies an early-exit optimization: if high-confidence contact data (phone number and physical address) is extracted in Tier 1, subsequent paid API tiers are bypassed, conserving API credits.

#### 3.5.4 Tasks Completed in Tax Intelligence Automation
* Designed, coded, and tested the modular architecture of `akirs-auto` utilizing `uv` for lightning-fast Python virtual environment and dependency management.
* Implemented resilient Playwright browser interaction handlers with automated DOM retries, randomized delays, and scroll event dispatchers to bypass dynamic infinite-scrolling anti-bot defenses.
* Formulated robust regex extractors for Nigerian telephone numbers, filtering out invalid short-codes while normalizing formatting across international `+234` and national `080` prefixes.
* Generated consolidated tax audit intelligence datasets exported in standardized CSV format, directly providing the Tax Intelligence Unit with hundreds of verified commercial leads operating within the state.

---

### 3.6 TAXPAYER DATA CLEANING, DEDUPLICATION & BATCH ETL PIPELINES (`akirs-data-cleaner`)

```
   ┌───────────────────────────────────────────────┐
   │    COMMERCIAL BANK MONTHLY SPREADSHEETS       │
   │  Heterogeneous Columns · Duplicates · Invalids│
   └───────────────────────┬───────────────────────┘
                           │ Multipart File Upload
                           ▼
   ┌───────────────────────────────────────────────┐
   │          FASTAPI DATA CLEANING PORTAL         │
   │           (Interactive Web Dashboard)         │
   └───────────────────────┬───────────────────────┘
                           │ OpenPyXL Streaming Parse
                           ▼
   ┌─────────────────────────────────────────────────────────────┐
   │                 TRANSFORMATION & VALIDATION                 │
   ├─────────────────────────────────────────────────────────────┤
   │ 1. Heuristic Header Mapping (fuzzy matches TIN, NUBAN, etc.) │
   │ 2. Algorithmic Checksum Validation (10-Digit NUBAN & TIN)   │
   │ 3. Geographic LGA Filtering (e.g. Isolate "Uyo" Branches)   │
   │ 4. Deterministic Deduplication (NUBAN & BVN Collision Check)│
   └─────────────────────────────┬───────────────────────────────┘
                                 │
                                 ▼
   ┌───────────────────────────────────────────────┐
   │            CLEAN PRODUCTION CSV EXPORT        │
   │     Ingestion-Ready for State Tax Database    │
   └───────────────────────────────────────────────┘
```

#### 3.6.1 Batch Processing of Commercial Bank New Account Feeds
Commercial banks operating within Akwa Ibom State are mandated by statutory revenue regulations to submit periodic batch spreadsheets containing newly opened corporate and individual bank accounts. These feeds provide vital intelligence for discovering non-compliant businesses.

However, bank submissions arrived in heterogeneous, chaotic formats. Spreadsheets from different financial institutions varied wildly: column header titles were inconsistent (e.g., `"ACC_NUM"`, `"NUBAN"`, `"ACCOUNT_NO"`, `"CUSTOMER_TIN"`), phone numbers lacked leading zeroes due to Excel numeric parsing errors, branch location columns contained disparate entries, and duplicate records abounded. Manual cleaning using Microsoft Excel required days of labor-intensive effort, introduced severe human error, and delayed enforcement.

#### 3.6.2 FastAPI-Powered Ingestion Portal & OpenPyXL Parsing Engine
To eliminate this operational bottleneck, I built and deployed **`akirs-data-cleaner`**, an enterprise web application featuring:
* **Modern Backend Architecture:** Built on Python's **FastAPI** asynchronous web framework, utilizing Starlette and Pydantic for high-throughput request validation.
* **Streaming Excel Parser:** Utilizes **OpenPyXL** in read-only streaming mode to process large multi-megabyte Excel files (`.xlsx`) without exhausting server RAM.
* **Browser-Based UI:** An intuitive web interface built with HTML5, modern CSS, and JavaScript allowing non-technical tax officers to drag-and-drop spreadsheets, configure cleaning parameters visually, preview transformation statistics, and download standardized CSV exports.

#### 3.6.3 Algorithmic NUBAN & TIN Validation, Duplicate Resolution, and Branch Filtering
The core transformation engine executes four rigorous validation passes:
1. **Heuristic Header Mapping:** Automatically detects and maps divergent column header names to standardized target fields (`NUBAN`, `TAXPAYER_ID`, `FIRST_NAME`, `SURNAME`, `PHONE_1`, `BRANCH`) using fuzzy string similarity matching.
2. **Algorithmic NUBAN & TIN Verification:** Validates that account numbers strictly adhere to the Central Bank of Nigeria (CBN) 10-digit NUBAN structural format and checksum. Flags truncated numbers caused by spreadsheet scientific notation errors (`1.23E+09`).
3. **Geographic Branch Filtering:** Scans freeform branch location fields using text matching algorithms to filter out records belonging to other states while cleanly capturing local tax jurisdictions (e.g., isolating accounts registered at Uyo, Ikot Ekpene, or Eket branches).
4. **Deterministic Deduplication:** Identifies and resolves record duplicates based on primary financial keys (NUBAN and BVN). Users can choose between automated resolution (dropping subsequent duplicate occurrences) or interactive merging of conflicting contact fields.

#### 3.6.4 Tasks Completed in Data Cleaning & Normalization
* Developed both the interactive FastAPI web application (`bot.py`, `app/main.py`, `app/services/cleaner.py`) and specialized standalone migration scripts (`migrate_data.py`, `migrate2.py`) for processing multi-gigabyte quarterly datasets.
* Implemented automated telephone number sanitization algorithms, stripping whitespace, trailing hyphens, and scientific notation while restoring leading zeroes.
* Processed and normalized over 120,000 raw banking records for the Q3/Q4 tax cycles, reducing data cleaning processing time from 4 working days to under 15 seconds.
* Standardized output datasets into UTF-8 encoded, clean CSV schemas that integrated directly into the state tax database without import syntax errors.

---

### 3.7 MONITORING, ALERTING & HARDWARE FLEET MAINTENANCE
#### 3.7.1 Hardware Health Audits & Proactive Alerting
Datacenter continuity demands proactive operational maintenance. Working alongside senior infrastructure engineers, I conducted daily hardware audits:
* Monitored server power distribution units (PDUs) and uninterruptible power supply (UPS) battery health, tracking load percentages and ambient datacenter temperature (maintained at 19°C).
* Configured automated SNMP (Simple Network Management Protocol) alerting on the Sophos XGS firewall and Synology NAS, dispatching immediate notification emails upon detecting disk degradation, port drops, or power anomalies.
* Audited server syslog streams, identifying and mitigating unauthorized SSH brute-force attempts targeting perimeter gateway nodes.

#### 3.7.2 HP ProBook Engineering Fleet Provisioning & Endpoint Security
As part of the JRB rollout, modern **HP ProBook 840 G10** enterprise laptops were provisioned for audit teams and systems engineers. I assisted in:
* Executing automated unattended OS deployments of Windows 11 Enterprise.
* Enforcing Microsoft BitLocker full-disk hardware encryption tied to the laptop's TPM 2.0 (Trusted Platform Module) chip, guaranteeing that sensitive fiscal data remains cryptographically inaccessible in the event of device theft.
* Installing centralized endpoint antivirus protection and establishing enterprise zero-trust VPN configurations for remote field tax auditors.

---

### 3.8 VERSION CONTROL, DEVOPS & CONTAINERIZATION
#### 3.8.1 Git, GitHub & Distributed Code Collaboration
All internal software development at AKIRS is governed by strict version control best practices:
* Maintained internal software repositories using **Git** and **GitHub**, adhering to feature-branch workflows (`feature/header-parser`, `fix/nuban-checksum`).
* Drafted comprehensive documentation (`README.md`, `CHANGELOG.md`, `deploy.md`) and executed collaborative pull request (PR) code reviews, practicing surgical code modifications and test-driven validation.

#### 3.8.2 Docker Containerization & Microservice Environments
To ensure reproducible deployments across staging and production server environments, I containerized the internal software tools:
* Authored optimized, multi-stage **Dockerfiles** for the FastAPI application, utilizing lightweight Alpine/Debian-slim base images and non-root execution users to minimize security attack surfaces.
* Configured **Docker Compose** orchestration files managing web backends, background worker queues, and persistent local volumes for file processing.

---

### 3.9 SOFT SKILLS, CROSS-DISCIPLINARY COLLABORATION & TECHNICAL COMMUNICATION
Beyond pure systems engineering and programming, the SIWES attachment cultivated essential human and professional competencies:
* **Interdisciplinary Collaboration:** Worked closely with tax assessment auditors and legal officers, learning how to translate complex fiscal requirements and statutory tax laws into concrete software validation algorithms.
* **Technical Presentation:** Prepared and presented slide decks and live software demonstrations during weekly ICT technical review meetings, explaining system architecture, data cleaning metrics, and infrastructure uptime status to executive management.
* **Operational Discipline & Accountability:** Operating within a government datacenter instilled a high level of accountability. Every terminal command, firewall modification, and server restart had to be meticulously logged in change management registers, reinforcing the critical mindset required for enterprise mission-critical engineering.

---

### 3.10 CHAPTER THREE CONCLUSION
Chapter Three has detailed the extensive practical engineering experiences gained during the six-month industrial attachment at the Akwa Ibom State Internal Revenue Service (AKIRS) under the JRB Digital Infrastructure Programme. By actively participating in datacenter virtualization, enterprise network administration, storage management, automated OSINT tax discovery, and high-throughput data cleaning, I bridged theoretical academic concepts with high-stakes production execution.

However, operating within this multi-server, multi-VM datacenter environment also exposed a critical architectural vulnerability common to modern distributed infrastructures: **the fragility, latency, and single-point-of-failure (SPOF) risks inherent in traditional centralized ingress reverse proxies**. This real-world operational challenge directly inspired the design, engineering, and implementation of my comprehensive technical project—**BRIDGE**—which is fully detailed in Chapter Four.

<div style="page-break-after: always;"></div>
"""
