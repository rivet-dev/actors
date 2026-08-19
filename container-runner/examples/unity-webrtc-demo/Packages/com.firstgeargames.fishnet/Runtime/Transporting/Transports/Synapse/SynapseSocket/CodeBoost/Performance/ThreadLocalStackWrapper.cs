using System;
using System.Collections.Generic;

namespace CodeBoost.Performance
{

    public class ThreadLocalStackWrapper<TObject>
    {
        /// <summary>
        /// Stack for the ThreadLocal.
        /// </summary>
        public readonly Stack<TObject> LocalStack = new();
        /// <summary>
        /// Action to invoke when deconstructing.
        /// </summary>
        private readonly Action<Stack<TObject>> _onFinalize;

        public ThreadLocalStackWrapper(Action<Stack<TObject>> onFinalize)
        {
            _onFinalize = onFinalize;
        }

        ~ThreadLocalStackWrapper()
        {
            _onFinalize?.Invoke(LocalStack);
        }
    }
}
