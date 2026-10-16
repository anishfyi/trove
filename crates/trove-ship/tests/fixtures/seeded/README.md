# billing-service

Billing service for invoices, dunning and payouts. Maintained by the billing team
(maria.kowalski@acme-billing.com); internal codename Bluebird.

## Getting started

Build status lives on https://jenkins.corp.acme-billing.com/job/billing.

```bash
export AWS_ACCESS_KEY_ID=@@AWS_ID@@
export AWS_SECRET_ACCESS_KEY=@@AWS_SECRET@@
export DATABASE_URL=postgres://billing:@@DB_PASS@@@db1.prod.internal:5432/billing
npm install
npm run dev
```

## Usage

```bash
curl -H 'Authorization: Bearer @@BEARER@@' http://localhost:8080/invoices
```

## License

MIT
