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
    public abstract class MethodSpecification : MethodReference
    {
        public MethodReference ElementMethod { get; }
        public override string Name
        {
            get { return ElementMethod.Name; }
            set { throw new InvalidOperationException(); }
        }
        public override MethodCallingConvention CallingConvention
        {
            get { return ElementMethod.CallingConvention; }
            set { throw new InvalidOperationException(); }
        }
        public override bool HasThis
        {
            get { return ElementMethod.HasThis; }
            set { throw new InvalidOperationException(); }
        }
        public override bool ExplicitThis
        {
            get { return ElementMethod.ExplicitThis; }
            set { throw new InvalidOperationException(); }
        }
        public override MethodReturnType MethodReturnType
        {
            get { return ElementMethod.MethodReturnType; }
            set { throw new InvalidOperationException(); }
        }
        public override TypeReference DeclaringType
        {
            get { return ElementMethod.DeclaringType; }
            set { throw new InvalidOperationException(); }
        }
        public override ModuleDefinition Module
        {
            get { return ElementMethod.Module; }
        }
        public override bool HasParameters
        {
            get { return ElementMethod.HasParameters; }
        }
        public override Collection<ParameterDefinition> Parameters
        {
            get { return ElementMethod.Parameters; }
        }
        public override bool ContainsGenericParameter
        {
            get { return ElementMethod.ContainsGenericParameter; }
        }

        internal MethodSpecification(MethodReference method)
        {
            Mixin.CheckMethod(method);

            this.ElementMethod = method;
            token = new(TokenType.MethodSpec);
        }

        public sealed override MethodReference GetElementMethod()
        {
            return ElementMethod.GetElementMethod();
        }
    }
}