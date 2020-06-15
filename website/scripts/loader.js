(function(Dprint) {
    if (document.readyState === "complete" || document.readyState === "interactive") {
        setTimeout(onLoad, 0);
    } else {
        document.addEventListener("DOMContentLoaded", onLoad);
    }

    function onLoad() {
        Dprint.replacePluginUrls();
        Dprint.replaceConfigTable();
        Dprint.addNavBurgerEvent();
    }
})(window.Dprint || (window.Dprint = {}));
