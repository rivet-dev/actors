//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;

namespace MonoFN.Cecil
{
    internal sealed class MemberDefinitionCollection<T> : Collection<T> where T : IMemberDefinition
    {
        private TypeDefinition container;

        internal MemberDefinitionCollection(TypeDefinition container)
        {
            this.container = container;
        }

        internal MemberDefinitionCollection(TypeDefinition container, int capacity) : base(capacity)
        {
            this.container = container;
        }

        protected override void OnAdd(T item, int index)
        {
            Attach(item);
        }

        protected sealed override void OnSet(T item, int index)
        {
            Attach(item);
        }

        protected sealed override void OnInsert(T item, int index)
        {
            Attach(item);
        }

        protected sealed override void OnRemove(T item, int index)
        {
            Detach(item);
        }

        protected sealed override void OnClear()
        {
            foreach (T definition in this)
                Detach(definition);
        }

        private void Attach(T element)
        {
            if (element.DeclaringType == container)
                return;

            if (element.DeclaringType != null)
                throw new ArgumentException("Member already attached");

            element.DeclaringType = container;
        }

        private static void Detach(T element)
        {
            element.DeclaringType = null;
        }
    }
}