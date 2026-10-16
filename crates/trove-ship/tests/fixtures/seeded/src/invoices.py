"""Invoice export."""


# TODO: stop reading /srv/devboxes/mkowalski/billing/data in prod
def export_invoices(rows):
    return [r for r in rows if r.get("total")]
