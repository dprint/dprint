---
title: Pricing
description: Information on how much dprint costs for commercial use.
---

# Pricing

Dprint's CLI will always be **free** for formatting open source projects whose primary maintainer is not a for-profit company.

If you are using dprint's CLI on a project whose primary maintainer is a for-profit company or individual, then you must purchase a license.

- _Open source_ - Free if the primary maintainer of the code being formatted is not a for-profit company.
- _Non-profit_ - Free for formatting code primarily maintained by a non-profit company.
- _Educational_ - Free for formatting code maintained by students or for educational purposes.
- _Commercial_ - Commercial license required by the primary maintainer if the primary maintainer is a for-profit company or individual.

## Questions and Answers

Do I need to purchase a license if...

1. ...I'm working on a codebase a commercial company maintains and they are using dprint?
   - **No.** It was/is the responsibility of the company maintaining the codebase to purchase one.
2. ...I'm working on a codebase a commercial company maintains and I want to introduce dprint?
   - **Talk to the company.** You need to talk to the company that maintains the code and they will need to purchase a license for dprint.
3. ...I'm a commercial user working on another entity's open source codebase.
   - **No.**
4. ...I'm a commercial user and I want to use dprint to format my company's code or my own commercial code.
   - **Yes.**
5. ...I work professionally as a freelancer and want to use it to format my open source side projects?
   - **No.**

## Commercial License

Commercial licenses are available as a site license depending on the number of developers and can be purchased below via PayPal:

<form id="pricing" action="https://www.paypal.com/cgi-bin/webscr" method="post" target="_top">
    <input type="hidden" name="cmd" value="_s-xclick">
    <input type="hidden" name="hosted_button_id" value="TN2W2MPLF5MBU">
    <input type="hidden" name="on0" value="">
    <select name="os0">
        <option value="Individual Professional">Individual Professional : $10.00 USD - monthly</option>
        <option value="Small Team (2-10)">Small Team (2-10) : $25.00 USD - monthly</option>
        <option value="Medium Team (11-25)">Medium Team (11-25) : $75.00 USD - monthly</option>
        <option value="Large Team (26-50)">Large Team (26-50) : $150.00 USD - monthly</option>
        <option value="Large Company (50+)">Large Company (50+) : $500.00 USD - monthly</option>
    </select>
    <input type="hidden" name="currency_code" value="USD">
    <input id="pricing-subscribe" type="image" src="/images/subscribe.png" border="0" name="submit" alt="Subscribe via PayPal.">
    <img alt="" border="0" src="https://www.paypalobjects.com/en_US/i/scr/pixel.gif" width="1" height="1">
</form>

## How to apply license?

Dprint doesn't use license keys in order to reduce friction. The only action you have to do to apply your license is update the project type property in your commercial project's configuration file (ex. _dprint.config.json_) to be `commercialPaid`.

<!-- dprint-ignore -->
```json
{
  "projectType": "commercialPaid",
  // etc...
}
```

Thank you for moving this project forward!
