export function addNavBurgerEvent() {
  const navBurger = document.getElementById("navbarBurger");
  navBurger.addEventListener("click", () => {
    navBurger.classList.toggle("is-active");
    document.getElementById(navBurger.dataset.target).classList.toggle("is-active");
  });
}
