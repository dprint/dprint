(function(Dprint) {
  Dprint.addNavBurgerEvent = function() {
    var navBurger = document.getElementById("navbarBurger");
    navBurger.addEventListener("click", function() {
      navBurger.classList.toggle("is-active");
      document.getElementById(navBurger.dataset.target).classList.toggle("is-active");
    });
  };
})(window.Dprint || (window.Dprint = {}));
