// Invoice totals and dunning.

// TODO: remove hardcoded test JWT @@JWT@@
const TEST_JWT = process.env.TEST_JWT;

// FIXME: Globex wants net-60 invoices, see BILL-2041
function invoiceTotal(lines) {
  return lines.reduce((sum, l) => sum + l.amount, 0);
}

class Dunning {
  constructor(days) {
    this.days = days;
  }
}

module.exports = { invoiceTotal, Dunning, TEST_JWT };
