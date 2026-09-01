using UnityEngine;
using UnityEngine.EventSystems;

namespace GameKit.Dependencies.Utilities
{
    public abstract class PointerMonoBehaviour : MonoBehaviour, IPointerEnterHandler, IPointerExitHandler, IPointerDownHandler, IPointerUpHandler
    {
        /// <summary>
        /// Called when the pointer enters this objects rect transform.
        /// </summary>
        public void OnPointerEnter(PointerEventData eventData) => OnHovered(true, eventData);

        /// <summary>
        /// Called when the pointer exits this objects rect transform.
        /// </summary>
        public void OnPointerExit(PointerEventData eventData) => OnHovered(false, eventData);

        /// <summary>
        /// Called when the pointer presses this objects rect transform.
        /// </summary>
        public void OnPointerDown(PointerEventData eventData) => OnPressed(true, eventData);

        /// <summary>
        /// Called when the pointer releases this objects rect transform.
        /// </summary>
        public void OnPointerUp(PointerEventData eventData) => OnPressed(false, eventData);

        public virtual void OnHovered(bool hovered, PointerEventData eventData) { }
        public virtual void OnPressed(bool pressed, PointerEventData eventData) { }
    }
}