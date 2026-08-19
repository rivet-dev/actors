using Sirenix.OdinInspector;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types
{
    public class CanvasGroupFader : MonoBehaviour
    {
        #region Types.
        /// <summary>
        /// Current fade state or goal for this class.
        /// </summary>
        public enum FadeGoalType
        {
            Unset = 0,
            Hidden = 1,
            Visible = 2
        }
        #endregion

        #region Public.
        /// <summary>
        /// Current goal for the fader.
        /// </summary>
        public FadeGoalType FadeGoal { get; private set; } = FadeGoalType.Unset;
        /// <summary>
        /// True if hidden or in the process of hiding.
        /// </summary>
        public bool IsHiding => FadeGoal == FadeGoalType.Hidden;
        /// <summary>
        /// True if visible. Will be true long as the CanvasGroup has alpha. Also see IsHiding.
        /// </summary>
        public bool IsVisible => CanvasGroup.alpha > 0f;
        #endregion

        #region Serialized.
        /// <summary>
        /// CanvasGroup to fade in and out.
        /// </summary>
        [Tooltip("CanvasGroup to fade in and out.")]
        [SerializeField]
        [TabGroup("Components")]
        protected CanvasGroup CanvasGroup;
        /// <summary>
        /// True to update the CanvasGroup blocking settings when showing and hiding.
        /// </summary>
        [Tooltip("True to update the CanvasGroup blocking settings when showing and hiding.")]
        [SerializeField]
        [TabGroup("Effects")]
        protected bool UpdateCanvasBlocking = true;
        /// <summary>
        /// How long it should take to fade in the CanvasGroup.
        /// </summary>
        [SerializeField]
        [TabGroup("Effects")]
        protected float FadeInDuration = 0.1f;
        /// <summary>
        /// How long it should take to fade out the CanvasGroup.
        /// </summary>
        [SerializeField]
        [TabGroup("Effects")]
        protected float FadeOutDuration = 0.3f;
        #endregion

        #region Private.
        /// <summary>
        /// True if a fade cycle has completed at least once.
        /// </summary>
        private bool _completedOnce;
        #endregion

        protected virtual void OnEnable()
        {
            FadeGoal = CanvasGroup.alpha > 0f ? FadeGoalType.Visible : FadeGoalType.Hidden;
        }

        protected virtual void OnDisable()
        {
            if (FadeGoal == FadeGoalType.Visible)
                ShowImmediately();
            else
                HideImmediately();
        }

        protected virtual void Update()
        {
            Fade();
        }

        /// <summary>
        /// Shows CanvasGroup immediately.
        /// </summary>
        public virtual void ShowImmediately()
        {
            SetFadeGoal(true);
            CompleteFade(true);
            OnShow();
        }

        /// <summary>
        /// Hides CanvasGroup immediately.
        /// </summary>
        public virtual void HideImmediately()
        {
            SetFadeGoal(false);
            CompleteFade(false);
            OnHide();
        }

        /// <summary>
        /// Shows CanvasGroup with a fade.
        /// </summary>
        public virtual void Show()
        {
            if (FadeInDuration <= 0f)
            {
                ShowImmediately();
            }
            else
            {
                SetFadeGoal(true);
                OnShow();
            }
        }

        /// <summary>
        /// Called after Show or ShowImmediate.
        /// </summary>
        protected virtual void OnShow() { }

        /// <summary>
        /// Hides CanvasGroup with a fade.
        /// </summary>
        public virtual void Hide()
        {
            if (FadeOutDuration <= 0f)
            {
                HideImmediately();
            }
            else
            {
                // Immediately make unclickable so players cannot hit UI objects as it's fading out.
                SetCanvasGroupBlockingType(CanvasGroupBlockingType.Block);
                SetFadeGoal(false);
                OnHide();
            }
        }

        /// <summary>
        /// Called after Hide or HideImmediate.
        /// </summary>
        protected virtual void OnHide() { }

        /// <summary>
        /// Sets showing and begins fading if required.
        /// </summary>
        /// <param name = "fadeIn"></param>
        private void SetFadeGoal(bool fadeIn)
        {
            FadeGoal = fadeIn ? FadeGoalType.Visible : FadeGoalType.Hidden;
        }

        /// <summary>
        /// Fades in or out over time.
        /// </summary>
        /// <returns></returns>
        private void Fade()
        {
            // Should not be possible.
            if (FadeGoal == FadeGoalType.Unset)
            {
                Debug.LogError($"{gameObject.name} has an unset FadeGoal. This should not be possible.");
                return;
            }

            bool fadingIn = FadeGoal == FadeGoalType.Visible;
            float duration;
            float targetAlpha;
            if (fadingIn)
            {
                targetAlpha = 1f;
                duration = FadeInDuration;
            }
            else
            {
                targetAlpha = 0f;
                duration = FadeOutDuration;
            }

            /* Already at goal and had completed an iteration at least once.
             * This is checked because even if at alpha we want to
             * complete the cycle if not done once so that all
             * local states and canvasgroup settings are proper. */
            if (_completedOnce && CanvasGroup.alpha == targetAlpha)
                return;

            float rate = 1f / duration;
            CanvasGroup.alpha = Mathf.MoveTowards(CanvasGroup.alpha, targetAlpha, rate * Time.deltaTime);

            // If complete.
            if (CanvasGroup.alpha == targetAlpha)
                CompleteFade(fadingIn);
        }

        /// <summary>
        /// Called when the fade completes.
        /// </summary>
        protected virtual void CompleteFade(bool fadingIn)
        {
            CanvasGroupBlockingType blockingType;
            float alpha;
            if (fadingIn)
            {
                blockingType = CanvasGroupBlockingType.Block;
                alpha = 1f;
            }
            else
            {
                blockingType = CanvasGroupBlockingType.DoNotBlock;
                alpha = 0f;
            }

            SetCanvasGroupBlockingType(blockingType);
            CanvasGroup.alpha = alpha;
            _completedOnce = true;
        }

        /// <summary>
        /// Changes the CanvasGroups interactable and bloacking state.
        /// </summary>
        protected virtual void SetCanvasGroupBlockingType(CanvasGroupBlockingType blockingType)
        {
            if (UpdateCanvasBlocking)
                CanvasGroup.SetBlockingType(blockingType);
        }
    }
}