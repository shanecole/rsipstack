import { NextRequest, NextResponse } from "next/server";
import { Grammar } from 'sip.js';
import { v4 } from 'uuid';
import { CreateSipClient, SipClient } from "@/src/utils/sip/sipclient";

const taskList = new Map<string, SipClient>();

export async function GET(req: NextRequest) {
    try {
        const { searchParams } = req.nextUrl;
        console.log(searchParams);
        const userName = searchParams.get("username") || '';
        const password  = searchParams.get("password") || '';
        const uri = Grammar.URIParse(searchParams.get("uri") || '');
        if (!uri) {
            return NextResponse.json({
                errcode: 2,
                errmsg: 'URI format error',
            });
        }
        console.log(uri);
        const taskId = v4();
        const client = CreateSipClient(taskId, uri, userName, password);
        if (!client) {
            return NextResponse.json({
                errcode: 3,
                errmsg: 'Unsupported transport type',
            });
        }
        taskList.set(taskId, client);
        client.run();
        return NextResponse.json({
            errcode: 0,
            errmsg: '',
            task_id: taskId
        });
    } catch (err) {
        console.log(err);
        return NextResponse.json({
            errcode: 1,
            errmsg: 'internal server error',
        });
    }
}
