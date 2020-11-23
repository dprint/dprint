---
title: Sponsor
description: Information on sponsoring dprint.
---

# Sponsor

Dprint's CLI will always be **free** for formatting open source projects whose primary maintainer is not a for-profit company.

If you are using dprint's CLI on a project whose primary maintainer is a for-profit company or individual, then there is a sponsorship requirement for the primary maintainer in order to use this software.

- _Open source_ - No sponsorship requirement if the primary maintainer of the code being formatted is not a for-profit company.
- _Non-profit_ - No sponsorship requirement for formatting code primarily maintained by a non-profit company.
- _Educational_ - No sponsorship requirement for formatting code maintained by students or for educational purposes.
- _Commercial_ - Primary maintainer must sponsor the project to use if the primary maintainer is a for-profit company or individual.

## Recommended Sponsorship Tier

You may select the sponsorship tier that you believe is fair taking into consideration:

1. The value dprint brings to your company and the number of developers it serves.
2. How much support your company wants to give to support this project's future development.

Recommended tier based on number of developers:

- Small Sponsorship: < 50 developers
- Medium Sponsorship: 50-99 developers
- Large Sponsorship: 100-199 developers
- Enterprise Sponsorship: 200+ developers

## PayPay

Sponsorship is available via PayPal:

<form id="sponsor" action="https://www.paypal.com/cgi-bin/webscr" method="post" target="_top">
   <input type="hidden" name="cmd" value="_s-xclick">
   <input type="hidden" name="hosted_button_id" value="3NURLRN43W9HE">
   <input type="hidden" name="on0" value="">
   <select name="os0">
      <option value="Minimal Sponsorship">Minimal Sponsorship : $10.00 USD - monthly</option>
      <option value="Small Sponsorship">Small Sponsorship : $20.00 USD - monthly</option>
      <option value="Sponsorship Tier 3">Sponsorship Tier 3 : $30.00 USD - monthly</option>
      <option value="Sponsorship Tier 4">Sponsorship Tier 4 : $50.00 USD - monthly</option>
      <option value="Medium Sponsorship">Medium Sponsorship : $75.00 USD - monthly</option>
      <option value="Sponsorship Tier 6">Sponsorship Tier 6 : $100.00 USD - monthly</option>
      <option value="Sponsorship Tier 7">Sponsorship Tier 7 : $150.00 USD - monthly</option>
      <option value="Large Sponsorship">Large Sponsorship : $250.00 USD - monthly</option>
      <option value="Sponsorship Tier 9">Sponsorship Tier 9 : $375.00 USD - monthly</option>
      <option value="Enterprise Sponsorship">Enterprise Sponsorship : $500.00 USD - monthly</option>
   </select>
   <input type="hidden" name="currency_code" value="USD">
   <input id="sponsor-subscribe" type="image" src="/images/subscribe.png" border="0" name="submit" alt="Sponsor via PayPal.">
   <img alt="" border="0" src="https://www.paypalobjects.com/en_US/i/scr/pixel.gif" width="1" height="1">
</form>

## GitHub Sponsors

Alternatively, some other sponsorship tiers are available through GitHub sponsors: [https://github.com/sponsors/dprint](https://github.com/sponsors/dprint)

## After Sponsoring

After you've sponsored, update your commercial project's configuration file (ex. _.dprintrc.json_) to be `commercialSponsored`.

<!-- dprint-ignore -->

```json
{
  "projectType": "commercialSponsored",
  // etc...
}
```

Thank you for moving this project forward!
